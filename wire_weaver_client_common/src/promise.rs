use crate::{Error, SeqTy};
use std::fmt::{Display, Formatter};
use tokio::sync::oneshot;
use tokio::sync::oneshot::error::TryRecvError;
use wire_weaver::prelude::DeserializeShrinkWrapOwned;

pub struct Promise<T> {
    state: PromiseState<T>,
    marker: &'static str,
    seen: bool,
}

#[derive(Default)]
pub enum PromiseState<T> {
    #[default]
    None,
    Waiting(SeqTy, oneshot::Receiver<Result<Vec<u8>, Error>>),
    Done(Option<T>), // Option used here to make Drop and take() work
    Err(Error),
}

impl<T: DeserializeShrinkWrapOwned> Promise<T> {
    pub fn empty(marker: &'static str) -> Self {
        Self {
            state: PromiseState::None,
            marker,
            seen: false,
        }
    }

    pub fn waiting(
        seq: SeqTy,
        rx: oneshot::Receiver<Result<Vec<u8>, Error>>,
        marker: &'static str,
    ) -> Self {
        Self {
            state: PromiseState::Waiting(seq, rx),
            marker,
            seen: false,
        }
    }

    pub fn error(error: Error, marker: &'static str) -> Self {
        Self {
            state: PromiseState::Err(error),
            marker,
            seen: false,
        }
    }

    /// Polls the promise and either returns a reference to the data or [None] if still pending.
    /// Note that error is also an option, but this method ignores it.
    pub fn ready(&mut self) -> Option<&T> {
        self.sync_poll();
        if let PromiseState::Done(response) = &self.state {
            response.as_ref()
        } else {
            None
        }
    }

    pub fn ready_mut(&mut self) -> Option<&mut T> {
        self.sync_poll();
        if let PromiseState::Done(response) = &mut self.state {
            response.as_mut()
        } else {
            None
        }
    }

    pub fn ready_if_unseen(&mut self) -> Option<&T> {
        self.sync_poll();
        if let PromiseState::Done(response) = &self.state {
            if self.seen {
                None
            } else {
                self.seen = true;
                response.as_ref()
            }
        } else {
            None
        }
    }

    pub fn take_ready(&mut self) -> Option<T> {
        self.sync_poll();
        if !matches!(self.state, PromiseState::Done(_)) {
            return None;
        }
        if let PromiseState::Done(ref mut response) = core::mem::take(&mut self.state) {
            response.take()
            // Some(response)
        } else {
            None
        }
    }

    pub fn take(&mut self) -> Result<Option<T>, String> {
        if let Some(r) = self.take_ready() {
            Ok(Some(r))
        } else if let Some(e) = self.peek_error() {
            self.state = PromiseState::None;
            Err(e)
        } else {
            Ok(None)
        }
    }

    pub fn peek_error(&mut self) -> Option<String> {
        self.sync_poll();
        if let PromiseState::Err(e) = &self.state {
            Some(format!("{e:?}"))
        } else {
            None
        }
    }

    pub fn sync_poll(&mut self) {
        if let PromiseState::Waiting(_seq, rx) = &mut self.state {
            match rx.try_recv() {
                Ok(response) => match response {
                    Ok(bytes) => match T::from_ww_bytes_owned(&bytes) {
                        Ok(reply) => {
                            self.state = PromiseState::Done(Some(reply));
                        }
                        Err(e) => {
                            self.state = PromiseState::Err(e.into());
                        }
                    },
                    Err(e) => {
                        self.state = PromiseState::Err(e);
                    }
                },
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Closed) => {
                    self.state = PromiseState::Err(Error::RxDispatcherNotRunning);
                }
            }
        }
    }
}

impl<T> Promise<T> {
    pub fn is_waiting(&self) -> bool {
        matches!(self.state, PromiseState::Waiting(_, _))
    }

    pub fn is_empty(&self) -> bool {
        matches!(self.state, PromiseState::None)
    }

    pub fn is_ready(&self) -> bool {
        matches!(self.state, PromiseState::Done(_))
    }

    pub fn state(&self) -> &PromiseState<T> {
        &self.state
    }

    pub fn marker(&self) -> &str {
        self.marker
    }
}

impl<T> Display for Promise<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Promise('{}')::", self.marker)?;
        match &self.state {
            PromiseState::None => write!(f, "None"),
            PromiseState::Waiting(seq, _) => write!(f, "Waiting(seq={seq})"),
            PromiseState::Done(_) => write!(f, "Done"),
            PromiseState::Err(e) => write!(f, "Err({e:?})"),
        }
    }
}

impl<T> Drop for Promise<T> {
    fn drop(&mut self) {
        if let PromiseState::Waiting(seq, _) = &self.state {
            tracing::warn!(
                "Dropping Promise(seq={}, marker='{}')::Waiting(T), likely an error",
                seq,
                self.marker
            );
        }
    }
}

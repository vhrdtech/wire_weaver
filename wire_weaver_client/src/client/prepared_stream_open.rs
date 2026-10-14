//! Opening a stream by calling its stream function (API-31, CLT-23): `RequestKind::StreamOpen`, answered with
//! `StreamOpened` and the function's result. See `docs/api/stream_reliability.md`.

use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::oneshot;
use wire_weaver::prelude::{DeserializeShrinkWrapOwned, UVlq32Backfill};
use wire_weaver::shrink_wrap::SerializeShrinkWrapOwned;
use wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned;
use ww_client_server::stream::StreamOpenedOwned;
use ww_client_server::{PathKindOwned, RequestKindOwned, RequestOwned};
use ww_stream::{Opened, StreamCursor};

use crate::client::positioned::{AckMode, PositionedConfig, PositionedStream, Shared};
use crate::event_loop::command::{Command, StreamOpenHook};
use crate::event_loop::commander::TransportCommander;
use crate::event_loop::rx_dispatcher::{ResponseReceiver, StreamUpdateReceiver};
use crate::{Error, Stream};

/// Result type of a stream function: `()` for `stream!` and functions without a result, `Result<A, E>` for
/// fallible ones. The stream is open only when it is `Ok`.
pub trait StreamOpenResult: DeserializeShrinkWrapOwned + Sized {
    /// What opening returns with the stream handle `S`: `S` for a stream without a result (`stream!`), or
    /// `Result<(A, S), E>`.
    type With<S>;
    /// Whether the device opened the stream.
    fn is_open(&self) -> bool;
    /// Join with the stream handle (dropped when not open).
    fn with<S>(self, stream: S) -> Self::With<S>;
}

impl StreamOpenResult for () {
    type With<S> = S;

    fn is_open(&self) -> bool {
        true
    }

    fn with<S>(self, stream: S) -> S {
        stream
    }
}

impl<A: DeserializeShrinkWrapOwned, E: DeserializeShrinkWrapOwned> StreamOpenResult
    for Result<A, E>
{
    type With<S> = Result<(A, S), E>;

    fn is_open(&self) -> bool {
        self.is_ok()
    }

    fn with<S>(self, stream: S) -> Result<(A, S), E> {
        self.map(|a| (a, stream))
    }
}

mod sealed {
    pub trait Sealed {}
}

/// A stream handle that opening a stream returns: [Stream] for plain streams, [PositionedStream] for counted and
/// reliable ones.
pub trait StreamHandle: sealed::Sealed + Sized {
    #[doc(hidden)]
    fn positioned() -> bool;
    #[doc(hidden)]
    fn from_parts(parts: HandleParts) -> Self;
}

#[doc(hidden)]
pub struct HandleParts(Parts);

enum Parts {
    Plain {
        transport_cmd_tx: TransportCommander,
        path_kind: PathKindOwned,
        rx: StreamUpdateReceiver,
    },
    Positioned {
        transport_cmd_tx: TransportCommander,
        path_kind: PathKindOwned,
        shared: Arc<Shared>,
    },
}

impl<T> sealed::Sealed for Stream<T> {}
impl<T> StreamHandle for Stream<T> {
    fn positioned() -> bool {
        false
    }

    fn from_parts(parts: HandleParts) -> Self {
        let Parts::Plain {
            transport_cmd_tx,
            path_kind,
            rx,
        } = parts.0
        else {
            unreachable!("checked when opening")
        };
        Stream {
            transport_cmd_tx,
            path_kind,
            rx,
            _phantom: PhantomData,
        }
    }
}

impl<T> sealed::Sealed for PositionedStream<T> {}
impl<T> StreamHandle for PositionedStream<T> {
    fn positioned() -> bool {
        true
    }

    fn from_parts(parts: HandleParts) -> Self {
        let Parts::Positioned {
            transport_cmd_tx,
            path_kind,
            shared,
        } = parts.0
        else {
            unreachable!("checked when opening")
        };
        PositionedStream {
            shared,
            transport_cmd_tx,
            path_kind,
            run: None,
            _phantom: PhantomData,
        }
    }
}

/// Opening a stream, from `Commander::prepare_stream_open` (`device.samples(config)` in generated code) or
/// `Commander::prepare_stream_resume` (`device.samples_resume(cursor)`). Does nothing until
/// [call](Self::call) or [blocking_call](Self::blocking_call).
#[must_use = "PreparedStreamOpen does nothing, unless call() or blocking_call() is used"]
pub struct PreparedStreamOpen<R, S> {
    pub(crate) postpone_err: Result<(), Error>,
    pub(crate) transport_cmd_tx: TransportCommander,
    pub(crate) path_kind: PathKindOwned,
    pub(crate) delivery: Option<ww_self::Delivery>,
    pub(crate) args: Vec<u8>,
    pub(crate) resume: Option<StreamCursor>,
    pub(crate) ack_mode: AckMode,
    pub(crate) window: u32,
    pub(crate) queue_bytes: usize,
    pub(crate) timeout_override: Option<Duration>,
    pub(crate) _phantom: PhantomData<(R, S)>,
}

impl<R: StreamOpenResult, S: StreamHandle> PreparedStreamOpen<R, S> {
    /// Use a provided timeout for the open request instead of the default one.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout_override = Some(timeout);
        self
    }

    /// When a reliable stream's data counts as received, so the device frees it ([AckMode::OnReceive] by default).
    pub fn ack_mode(mut self, mode: AckMode) -> Self {
        self.ack_mode = mode;
        self
    }

    /// Reliable streams: bytes the device may have in flight beyond what the host acknowledged, which is what the
    /// host queues for the stream at most ([DEFAULT_WINDOW](crate::DEFAULT_WINDOW)).
    pub fn window(mut self, bytes: u32) -> Self {
        self.window = bytes;
        self
    }

    /// Plain and counted streams: bytes of items queued before new ones are dropped (and reported as a
    /// `Gap { cause: Host }`), [ClientConfig::stream_queue_bytes](crate::ClientConfig::stream_queue_bytes) by
    /// default.
    pub fn queue_bytes(mut self, bytes: usize) -> Self {
        self.queue_bytes = bytes;
        self
    }

    /// Open the stream: the stream function's result with the stream handle (see [StreamOpenResult]).
    pub async fn call(mut self) -> Result<R::With<S>, Error> {
        std::mem::replace(&mut self.postpone_err, Ok(()))?;
        let plain_rx = if S::positioned() || self.delivery.is_some() {
            None
        } else {
            Some(
                self.transport_cmd_tx
                    .register_stream(&self.path_kind, self.queue_bytes)
                    .await?,
            )
        };
        let (parts, (cmd, done_rx)) = self.start(plain_rx)?;
        self.transport_cmd_tx.send_command(cmd).await?;
        let response = done_rx.await.map_err(|_| Error::RxDispatcherNotRunning);
        finish::<R, S>(parts, response)
    }

    /// Blocking variant of [Self::call].
    pub fn blocking_call(mut self) -> Result<R::With<S>, Error> {
        std::mem::replace(&mut self.postpone_err, Ok(()))?;
        let plain_rx = if S::positioned() || self.delivery.is_some() {
            None
        } else {
            Some(
                self.transport_cmd_tx
                    .register_stream_blocking(&self.path_kind, self.queue_bytes)?,
            )
        };
        let (parts, (cmd, done_rx)) = self.start(plain_rx)?;
        self.transport_cmd_tx.send_command_blocking(cmd)?;
        let response = done_rx
            .blocking_recv()
            .map_err(|_| Error::RxDispatcherNotRunning);
        finish::<R, S>(parts, response)
    }

    /// Checks, the open request and what it needs (a plain stream's receiver, registered before the request goes
    /// out so that nothing the device sends after opening is lost; a positioned stream's state).
    fn start(
        &self,
        plain_rx: Option<StreamUpdateReceiver>,
    ) -> Result<(Parts, (Command, ResponseReceiver)), Error> {
        if S::positioned() != self.delivery.is_some() {
            return Err(Error::User(format!(
                "stream at {:?} is {}, opened as {}",
                self.path_kind,
                match self.delivery {
                    Some(d) => format!("{d:?}"),
                    None => "plain".into(),
                },
                if S::positioned() {
                    "a PositionedStream"
                } else {
                    "a plain Stream"
                },
            )));
        }
        let (done_tx, done_rx) = oneshot::channel();
        let request = RequestOwned {
            seq: UVlq32Backfill(0),
            path_kind: self.path_kind.clone(),
            kind: RequestKindOwned::StreamOpen {
                resume: self.resume,
                args: TailBytesOwned(self.args.clone()),
            },
        }
        .to_ww_bytes_owned()?;
        let timeout = self
            .timeout_override
            .unwrap_or(self.transport_cmd_tx.default_timeout());
        let Some(delivery) = self.delivery else {
            let cmd = Command::SendMessage {
                bytes: request,
                done_tx: Some((done_tx, timeout)),
            };
            let parts = Parts::Plain {
                transport_cmd_tx: self.transport_cmd_tx.clone(),
                path_kind: self.path_kind.clone(),
                rx: plain_rx.expect("registered for a plain stream"),
            };
            return Ok((parts, (cmd, done_rx)));
        };
        let PathKindOwned::Absolute { path } = &self.path_kind else {
            // TODO(CLT-23): events carry the absolute path, a trait path has to be resolved first
            return Err(Error::User(
                "counted and reliable streams can only be opened on an absolute path yet".into(),
            ));
        };
        let shared = Shared::new(
            path.clone(),
            PositionedConfig {
                delivery,
                ack_mode: self.ack_mode,
                window: self.window,
                queue_bytes: self.queue_bytes,
            },
        );
        let cmd = Command::OpenStream {
            bytes: request,
            done_tx,
            timeout,
            hook: Box::new(StreamOpenHook::new::<R>(shared.clone(), self.resume)),
        };
        let parts = Parts::Positioned {
            transport_cmd_tx: self.transport_cmd_tx.clone(),
            path_kind: self.path_kind.clone(),
            shared,
        };
        Ok((parts, (cmd, done_rx)))
    }
}

/// Decode the device's answer to an open: `StreamOpened` with the stream function's result in it.
pub(crate) fn decode_opened(bytes: &[u8]) -> Result<(Opened, Vec<u8>), Error> {
    let opened = StreamOpenedOwned::from_ww_bytes_owned(bytes)?;
    let result = opened.result.0.clone();
    Ok((
        Opened {
            session: opened.session.0,
            oldest: opened.oldest.0,
            live_from: opened.live_from.0,
            resume_from: opened.resume_from.map(|p| p.0),
        },
        result,
    ))
}

/// For the event loop: whether an answer to an open says the stream is open (decodes `R`).
pub(crate) fn is_open<R: StreamOpenResult>(bytes: &[u8]) -> bool {
    decode_opened(bytes)
        .and_then(|(_, result)| Ok(R::from_ww_bytes_owned(&result)?))
        .is_ok_and(|r| r.is_open())
}

fn finish<R: StreamOpenResult, S: StreamHandle>(
    parts: Parts,
    response: Result<Result<Vec<u8>, Error>, Error>,
) -> Result<R::With<S>, Error> {
    let decoded = response.and_then(|r| r).and_then(|bytes| {
        let (_, result) = decode_opened(&bytes)?;
        Ok(R::from_ww_bytes_owned(&result)?)
    });
    let result = match decoded {
        Ok(r) if r.is_open() => {
            if let Parts::Positioned {
                transport_cmd_tx,
                shared,
                ..
            } = &parts
            {
                let mut streams = transport_cmd_tx
                    .streams
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                streams.retain(|s| s.strong_count() > 0);
                streams.push(Arc::downgrade(shared));
            }
            r
        }
        Ok(r) => {
            discard(&parts);
            return Ok(r.with(S::from_parts(HandleParts(parts))));
        }
        Err(e) => {
            discard(&parts);
            return Err(e);
        }
    };
    Ok(result.with(S::from_parts(HandleParts(parts))))
}

/// The stream didn't open: its handle goes without closing it on the device.
fn discard(parts: &Parts) {
    if let Parts::Positioned { shared, .. } = parts {
        shared.lock().closed = true;
    }
}

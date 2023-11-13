use std::fmt::{Debug, Formatter};
use std::io::Cursor;

use serde::Deserialize;
use tracing::{trace, warn};

use xpi::client_server_owned::{EventKind, Nrl, ReplyKind, RequestId, RequestKind};
use xpi::error::XpiError;

use crate::client::Client;

// TODO: add timeout check
#[derive(PartialEq, Default)]
pub enum PromiseStream<T> {
    #[default]
    None,
    Waiting {
        rid: RequestId,
        nrl: Nrl,
    },
    Streaming {
        rid: RequestId,
        nrl: Nrl,
        items: Vec<T>,
    },
    Done {
        remaining_items: Vec<T>,
    },
    Err(XpiError),
}

impl<'de, T: Deserialize<'de>> PromiseStream<T> {
    /// Polls the client for new data for this Promise.
    /// Returns true if changes were made (reply or error received).
    pub fn poll(&mut self, client: &mut Client) -> bool {
        let rid = match self {
            PromiseStream::Waiting { rid, .. } => *rid,
            PromiseStream::Streaming { rid, .. } => *rid,
            _ => {
                return false;
            }
        };
        let mut changed = false;
        for ev in client.poll_all(rid) {
            let EventKind::Reply { result } = ev.kind else { continue; };
            let reply_kind = match result {
                Ok(k) => k,
                Err(e) => {
                    warn!("PromiseStream {rid:?}: got error: {e:?}");
                    *self = PromiseStream::Err(e);
                    return true;
                }
            };
            match reply_kind {
                ReplyKind::StreamOpened => match core::mem::take(self) {
                    PromiseStream::Waiting { nrl, .. } => {
                        *self = PromiseStream::Streaming {
                            rid,
                            nrl,
                            items: Vec::new(),
                        };
                        changed = true;
                    }
                    PromiseStream::Streaming { rid, nrl, items } => {
                        warn!("PromiseStream {rid:?}: got StreamOpened when already streaming");
                        *self = PromiseStream::Streaming { rid, nrl, items };
                    }
                    PromiseStream::Done { remaining_items } => {
                        warn!("PromiseStream {rid:?}: got StreamOpened in Done state");
                        *self = PromiseStream::Done { remaining_items };
                    }
                    PromiseStream::Err(e) => {
                        warn!("PromiseStream {rid:?}: got error: {e:?}");
                        *self = PromiseStream::Err(e.clone());
                    }
                    PromiseStream::None => {}
                },
                ReplyKind::StreamUpdate { data } => {
                    let len = data.len();
                    let cur = Cursor::new(data);
                    let mut de = rmp_serde::Deserializer::new(cur);
                    let new_items: Vec<T> = Deserialize::deserialize(&mut de).unwrap();
                    trace!(
                        "{rid:?} got promised stream items for {:?} ({}B {} items)",
                        ev.seq,
                        len,
                        new_items.len()
                    );
                    match core::mem::take(self) {
                        PromiseStream::Streaming {
                            rid,
                            nrl,
                            mut items,
                        } => {
                            items.extend(new_items);
                            *self = PromiseStream::Streaming { rid, nrl, items };
                            changed = true;
                        }
                        PromiseStream::Waiting { nrl, .. } => {
                            *self = PromiseStream::Streaming {
                                rid,
                                nrl,
                                items: new_items,
                            };
                            changed = true;
                        }
                        PromiseStream::Done { remaining_items } => {
                            warn!(
                                "PromiseStream {rid:?}: got more items after StreamClosed or Error"
                            );
                            *self = PromiseStream::Done { remaining_items };
                        }
                        PromiseStream::Err(e) => {
                            *self = PromiseStream::Err(e);
                        }
                        PromiseStream::None => {}
                    }
                }
                ReplyKind::StreamClosed => match core::mem::take(self) {
                    PromiseStream::Streaming { items, .. } => {
                        *self = PromiseStream::Done {
                            remaining_items: items,
                        };
                    }
                    PromiseStream::Waiting { .. } => {
                        *self = PromiseStream::Done {
                            remaining_items: Vec::new(),
                        };
                    }
                    PromiseStream::Done { remaining_items } => {
                        *self = PromiseStream::Done { remaining_items };
                    }
                    PromiseStream::None => {}
                    PromiseStream::Err(e) => {
                        warn!("PromiseStream: got error: {e:?} after stream was already closed");
                        *self = PromiseStream::Err(e);
                        changed = true;
                    }
                },
                e => warn!("PromiseStream {rid:?}: unexpected event: {e}"),
            }
        }
        changed
    }

    pub fn drain(&mut self) -> Vec<T> {
        match core::mem::take(self) {
            PromiseStream::Waiting { rid, nrl } => {
                *self = PromiseStream::Waiting { rid, nrl };
                Vec::new()
            }
            PromiseStream::Streaming { rid, nrl, items } => {
                *self = PromiseStream::Streaming {
                    rid,
                    nrl,
                    items: Vec::new(),
                };
                items
            }
            PromiseStream::Done { remaining_items } => {
                *self = PromiseStream::Done {
                    remaining_items: Vec::new(),
                };
                remaining_items
            }
            PromiseStream::Err(e) => {
                *self = PromiseStream::Err(e);
                Vec::new()
            }
            PromiseStream::None => Vec::new(),
        }
    }

    /// Drop all received items except the last one and return a reference to it.
    /// Useful when receiving long operation updates and displaying spinner + progress info.
    pub fn drain_last(&mut self) -> Option<&T> {
        match self {
            PromiseStream::Waiting { .. } => None,
            PromiseStream::Streaming { items, .. }
            | PromiseStream::Done {
                remaining_items: items,
                ..
            } => {
                if items.len() >= 2 {
                    items.drain(0..items.len() - 2);
                }
                items.last()
            }
            PromiseStream::Err(_) => None,
            PromiseStream::None => None,
        }
    }

    /// Drop all received items except the last one and return a reference to it.
    /// Useful when receiving long operation updates and displaying spinner + progress info.
    pub fn drain_and_take_last(&mut self) -> Option<T> {
        match self {
            PromiseStream::Waiting { .. } => None,
            PromiseStream::Streaming { items, .. }
            | PromiseStream::Done {
                remaining_items: items,
                ..
            } => {
                if items.len() >= 2 {
                    items.drain(0..items.len() - 2);
                }
                items.pop()
            }
            PromiseStream::Err(_) => None,
            PromiseStream::None => None,
        }
    }

    pub fn peek(&self) -> Option<&[T]> {
        match self {
            PromiseStream::None => None,
            PromiseStream::Waiting { .. } => None,
            PromiseStream::Streaming { items, .. } => Some(&items),
            PromiseStream::Done {
                remaining_items, ..
            } => Some(&remaining_items),
            PromiseStream::Err(_) => None,
        }
    }

    pub fn unsubscribe(&mut self, client: &mut Client) {
        match self {
            PromiseStream::Streaming { nrl, .. } | PromiseStream::Waiting { nrl, .. } => {
                let _ = client.send_request(nrl.clone(), RequestKind::CloseStream);
            }
            _ => {}
        }
    }

    /// Returns true if this Promise can be overwritten (None or Err state)
    pub fn is_passive(&self) -> bool {
        match self {
            PromiseStream::None | PromiseStream::Err(_) => true,
            PromiseStream::Waiting { .. } | PromiseStream::Streaming { .. } => false,
            PromiseStream::Done { remaining_items } => remaining_items.is_empty(),
        }
    }

    pub fn is_none(&self) -> bool {
        matches!(self, PromiseStream::None)
    }

    pub fn is_waiting(&self) -> bool {
        matches!(self, PromiseStream::Waiting { .. })
    }

    pub fn is_waiting_or_streaming(&self) -> bool {
        matches!(
            self,
            PromiseStream::Waiting { .. } | PromiseStream::Streaming { .. }
        )
    }

    pub fn is_streaming(&self) -> bool {
        matches!(self, PromiseStream::Streaming { .. })
    }

    pub fn is_done(&self) -> bool {
        matches!(self, PromiseStream::Done { .. })
    }

    pub fn is_err(&self) -> bool {
        matches!(self, PromiseStream::Err { .. })
    }

    pub fn clear(&mut self) {
        if !self.is_passive() {
            warn!("Clearing non-passive PromiseStream, please unsubscribe first");
        }
        *self = PromiseStream::None;
    }
}

impl<T> Debug for PromiseStream<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            PromiseStream::None => write!(f, "PromiseStream: None"),
            PromiseStream::Waiting { rid, .. } => write!(f, "PromiseStream: {rid:?}: Waiting"),
            PromiseStream::Streaming { rid, .. } => write!(f, "PromiseStream: {rid:?}: Streaming"),
            PromiseStream::Done { .. } => write!(f, "PromiseStream: Done"),
            PromiseStream::Err(e) => write!(f, "PromiseStream: {e:?}"),
        }
    }
}

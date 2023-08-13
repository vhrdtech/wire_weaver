use std::fmt::Debug;
use std::io::Cursor;

use serde::Deserialize;
use tracing::trace;

use xpi::client_server_owned::{EventKind, ReplyKind, RequestId};
use xpi::error::XpiError;

use crate::client::Client;

// TODO: add timeout check
// TODO: handle local error instead of unwraps
#[derive(PartialEq, Default, Debug)]
pub enum Promise<T> {
    #[default]
    None,
    Waiting(RequestId),
    Done(T),
    Err(XpiError),
}

impl<'de, T: Deserialize<'de> + Debug> Promise<T> {
    /// Polls the client for new data for this Promise.
    /// Returns true if changes were made (reply or error received).
    pub fn poll(&mut self, client: &mut Client) -> bool {
        if let Promise::Waiting(rid) = self {
            if let Some(ev) = client.poll_one(*rid) {
                // trace!("got promised answer {ev:?}");
                if let EventKind::Reply { result } = ev.kind {
                        match result {
                            Ok(r) => {
                                if let ReplyKind::ReturnValue { data } = r {
                                    let cur = Cursor::new(data);
                                    let mut de = rmp_serde::Deserializer::new(cur);
                                    let val: T = Deserialize::deserialize(&mut de).unwrap();
                                    trace!("got promised answer {val:?}");
                                    *self = Promise::Done(val)
                                } else {
                                    *self = Promise::Err(XpiError::Internal);
                                }
                            }
                            Err(e) => {
                                *self = Promise::Err(e.clone());
                            }
                        }
                    return true;
                }
            }
        }
        false
    }
}
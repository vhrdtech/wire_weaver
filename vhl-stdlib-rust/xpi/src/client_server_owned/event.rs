use core::fmt::Display;

use crate::error::XpiError;

use super::{Nrl, Protocol, Reply, ReplyAck, Request, RequestId, RequestKind};
use smallvec::{smallvec, SmallVec};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Event {
    // pub source: Option<Address>,
    // pub destination: Option<Address>,
    // pub base_nrl: Option<Nrl>,
    pub kind: EventKind,
    pub seq: RequestId,
}

#[derive(Clone, Debug)]
pub struct AddressableEvent {
    pub protocol: Protocol,
    pub is_inbound: bool,
    pub event: Event,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum EventKind {
    Request {
        // If multiple actions are targeted at one sub level
        // common_nrl: Option<Nrl>,
        actions: SmallVec<[Request; 1]>,
        bail_on_error: bool,
    },
    Reply {
        results: SmallVec<[Reply; 1]>,
    },
}

impl Event {
    pub fn flip_with_error(&self, err: XpiError) -> Option<Event> {
        match &self.kind {
            EventKind::Request { actions, .. } => {
                let kind = EventKind::Reply {
                    results: actions.iter().map(|a| a.flip_with_error(err)).collect(),
                };
                Some(Event {
                    // source: self.destination.clone(),
                    // destination: self.source.clone(),
                    kind,
                    seq: self.seq,
                })
            }
            EventKind::Reply { .. } => None,
        }
    }

    pub fn new_heartbeat(
        // source: Option<Address>,
        // destination: Option<Address>,
        seq: RequestId,
    ) -> Self {
        Event {
            // source,
            // destination,
            kind: EventKind::Request {
                actions: smallvec![Request {
                    tr: None,
                    nrl: Nrl::default(),
                    reply_ack: ReplyAck::Ack,
                    kind: RequestKind::Ping
                }],
                bail_on_error: false,
            },
            seq,
        }
    }

    pub fn request_single(action: Request, seq: RequestId) -> Self {
        Event {
            kind: EventKind::Request {
                actions: smallvec![action],
                bail_on_error: true,
            },
            seq,
        }
    }
}

impl AddressableEvent {
    pub fn prepare_reply(&self, reserve_len: usize) -> AddressableEvent {
        let mut results = SmallVec::new();
        results.reserve(reserve_len);
        AddressableEvent {
            protocol: self.protocol,
            is_inbound: false,
            event: Event {
                kind: EventKind::Reply { results },
                seq: self.event.seq,
            },
        }
    }
}

impl Display for Event {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match &self.kind {
            EventKind::Request {
                actions,
                bail_on_error,
            } => {
                if actions.len() == 1 {
                    write!(f, "{}@{}", actions[0], self.seq.0)?;
                } else {
                    write!(f, "{}[", actions.len())?;
                    for req in actions {
                        write!(f, "{req}, ")?;
                    }
                    write!(f, "]")?;
                    if *bail_on_error {
                        write!(f, "+bail_on_error")?;
                    }
                }
            }
            EventKind::Reply { results } => {
                if results.len() == 1 {
                    write!(f, "{}@{}", results[0], self.seq.0)?;
                } else {
                    write!(f, "{}[", results.len())?;
                    for res in results {
                        write!(f, "{res}, ")?;
                    }
                    write!(f, "]")?;
                }
            }
        }
        Ok(())
    }
}

impl Display for AddressableEvent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.is_inbound {
            write!(f, "rx_")?;
        } else {
            write!(f, "tx_")?;
        }
        write!(f, "{} {}", self.protocol, self.event)
    }
}

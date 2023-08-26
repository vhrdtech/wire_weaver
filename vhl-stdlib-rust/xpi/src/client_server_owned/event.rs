use core::fmt::Display;
use futures::channel::mpsc::Sender;

use crate::error::XpiError;

use super::{Nrl, Protocol, ReplyKind, RequestId, RequestKind};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Event {
    // pub source: Option<Address>,
    // pub destination: Option<Address>,
    // pub base_nrl: Option<Nrl>,
    pub nrl: Nrl,
    pub kind: EventKind,
    pub seq: RequestId,
}

#[derive(Clone, Debug)]
pub struct AddressableEvent {
    pub protocol: Protocol,
    pub is_inbound: bool,
    pub event: Event,
    pub response_tx: Sender<AddressableEvent>,
}

impl AddressableEvent {
    pub fn from_req_and_kind(ev: &AddressableEvent, kind: EventKind) -> Self {
        AddressableEvent {
            protocol: ev.protocol,
            is_inbound: false,
            event: Event {
                nrl: ev.event.nrl.clone(),
                kind,
                seq: ev.event.seq,
            },
            response_tx: ev.response_tx.clone(),
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum EventKind {
    // Request {
    //     // If multiple actions are targeted at one sub level
    //     // common_nrl: Option<Nrl>,
    //     actions: SmallVec<[Request; 1]>,
    //     bail_on_error: bool,
    // },
    // Reply {
    //     results: SmallVec<[Reply; 1]>,
    // },
    Request { kind: RequestKind },
    Reply { result: Result<ReplyKind, XpiError> },
}

impl Event {
    pub fn flip_with_error(&self, err: XpiError) -> Option<Event> {
        match &self.kind {
            EventKind::Request { .. } => Some(Event {
                nrl: self.nrl.clone(),
                kind: EventKind::Reply { result: Err(err) },
                seq: self.seq,
            }),
            EventKind::Reply { .. } => None,
        }
    }

    pub fn call(nrl: Nrl, args: Vec<u8>, seq: RequestId) -> Self {
        Event {
            nrl,
            kind: EventKind::Request {
                kind: RequestKind::Call { args },
            },
            seq,
        }
    }

    pub fn stream_update(nrl: Nrl, data: Vec<u8>, seq: RequestId) -> Self {
        Event {
            nrl,
            kind: EventKind::Reply {
                result: Ok(ReplyKind::StreamUpdate { data }),
            },
            seq,
        }
    }

    pub fn stream_closed(nrl: Nrl, seq: RequestId) -> Self {
        Event {
            nrl,
            kind: EventKind::Reply {
                result: Ok(ReplyKind::StreamClosed),
            },
            seq,
        }
    }

    pub fn heartbeat(seq: RequestId) -> Self {
        Event {
            nrl: Nrl::default(),
            kind: EventKind::Request {
                kind: RequestKind::Ping,
            },
            seq,
        }
    }

    // pub fn request(kind: RequestKind, seq: RequestId) -> Self {
    //     Event {
    //         kind,
    //         seq,
    //     }
    // }
}

// impl AddressableEvent {
//     pub fn prepare_reply(&self, reserve_len: usize) -> AddressableEvent {
//         let mut results = SmallVec::new();
//         results.reserve(reserve_len);
//         AddressableEvent {
//             protocol: self.protocol,
//             is_inbound: false,
//             event: Event {
//                 kind: EventKind::Reply { results },
//                 seq: self.event.seq,
//             },
//         }
//     }
// }

impl Display for Event {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match &self.kind {
            EventKind::Request { kind } => {
                write!(f, "{}:{kind} {:?}", self.nrl, self.seq)
            }
            EventKind::Reply { result } => match result {
                Ok(result) => write!(f, "{}: {result} {:?}", self.nrl, self.seq),
                Err(e) => write!(f, "{}: {e:?} {:?}", self.nrl, self.seq),
            },
        }
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

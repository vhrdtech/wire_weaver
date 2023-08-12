pub mod error;
pub mod ws;

use error::Error;
use std::{collections::HashMap, fmt::Display, time::Instant};
use tokio::sync::mpsc::{
    error::TryRecvError, unbounded_channel, UnboundedReceiver, UnboundedSender,
};
use tracing::{error, trace, warn};
use xpi::client_server_owned::{Event, EventKind, Nrl, Protocol, ReplyKind, RequestId, RequestKind};
use xpi::error::XpiError;

pub mod prelude {
    pub use super::error::Error as NodeError;
    pub use super::Client;
    pub use rmp_serde;
    pub use smallvec::smallvec;
    pub use std::io::Cursor;
    pub use xpi::client_server_owned::prelude::*;
    pub use xpi::error::XpiError;
}

pub struct ClientManager {
    seq_subset: u8,
    tx_events: UnboundedSender<Event>,
    tx_internal: UnboundedSender<InternalReq>,
    rx_internal: UnboundedReceiver<InternalResp>,
}

#[derive(Debug)]
pub enum InternalReq {
    AddInstance {
        seq_subset: u8,
        tx: UnboundedSender<Event>,
        name: String,
    },
    Connect(Protocol),
    Disconnect,
    Stop,
}

pub enum InternalResp {
    InstanceCreated,
}

impl ClientManager {
    pub fn new() -> (ClientManager, impl std::future::Future<Output = ()>) {
        let (tx_events, rx_events) = unbounded_channel();
        let (tx_self, rx_router) = unbounded_channel();
        let (tx_router, rx_self) = unbounded_channel();

        let event_loop = ws::ws_event_loop(rx_events, rx_router, tx_router);

        (
            ClientManager {
                seq_subset: 0,
                tx_events,
                tx_internal: tx_self,
                rx_internal: rx_self,
            },
            event_loop,
        )
    }

    pub fn blocking_split<S: AsRef<str>>(&mut self, debug_name: S) -> Result<Client, Error> {
        let (tx_router, rx_node) = unbounded_channel();

        let seq_subset = self.seq_subset;
        self.seq_subset += 1; // TODO: handle overflow
        self.tx_internal
            .send(InternalReq::AddInstance {
                seq_subset,
                tx: tx_router,
                name: debug_name.as_ref().to_owned(),
            })
            .map_err(|_| Error::SplitFailed)?;
        let Some(InternalResp::InstanceCreated) = self.rx_internal.blocking_recv() else {
            return Err(Error::SplitFailed);
        };
        Ok(Client {
            seq_subset,
            seq: 0,
            tx: self.tx_events.clone(),
            rx: rx_node,
            rx_flatten: HashMap::new(),
            status: ClientStatus::default(),
            seq_status: HashMap::new(),
        })
    }

    pub fn connect(&mut self, protocol: Protocol) -> Result<(), Error> {
        // let addr = Address::parse(addr).unwrap();
        self.tx_internal
            .send(InternalReq::Connect(protocol))
            .unwrap();
        Ok(())
    }

    pub fn disconnect_and_stop(&mut self) {
        self.tx_internal.send(InternalReq::Disconnect).ok();
        self.tx_internal.send(InternalReq::Stop).ok();
    }
}

pub struct Client {
    seq_subset: u8,
    seq: u32,
    tx: UnboundedSender<Event>,
    rx: UnboundedReceiver<Event>,
    rx_flatten: HashMap<RequestId, Vec<Event>>,
    status: ClientStatus,
    seq_status: HashMap<RequestId, SeqStatus>,
}

impl Client {
    pub fn status(&self) -> ClientStatus {
        self.status
    }

    fn recycle_request_id(&mut self, ev: &Event) {
        // TODO: remove old request ids
        if let EventKind::Reply { result } = &ev.kind {
            // for reply in results {
                match &result {
                    Ok(kind) => match kind {
                        ReplyKind::StreamOpened
                        | ReplyKind::StreamUpdate { .. }
                        | ReplyKind::Subscribed => match self.seq_status.get_mut(&ev.seq) {
                            Some(status) => {
                                *status = SeqStatus::Streaming {
                                    last_update: Instant::now(),
                                }
                            }
                            None => {
                                warn!("Stream update for {} with seq: {:?} received without prior request, probably a bug", ev.nrl, ev.seq);
                            }
                        },
                        _ => match self.seq_status.get_mut(&ev.seq) {
                            Some(status) => {
                                if let SeqStatus::Done { .. } = *status {
                                    warn!("Got a second reply for {:?}?", ev.seq);
                                }
                                *status = SeqStatus::Done {
                                    since: Instant::now(),
                                };
                            }
                            None => {
                                warn!("Got reply for an unknown request: {:?}", ev.seq);
                            }
                        },
                    },
                    Err(_) => {}
                }
            // }
        }
    }

    pub fn next_request_id(&mut self) -> RequestId {
        let rid = loop {
            let rid = RequestId(((self.seq_subset as u32) << 24) + self.seq);
            self.seq = self.seq.wrapping_add(1);
            if self.seq_status.contains_key(&rid) {
                continue;
            }
            break rid;
        };
        self.seq_status.insert(
            rid,
            SeqStatus::AwaitingReply {
                created: Instant::now(),
            },
        );
        rid
    }

    pub fn receive_events(&mut self) {
        loop {
            match self.rx.try_recv() {
                Ok(ev) => {
                    self.recycle_request_id(&ev);
                    let bucket = self.rx_flatten.entry(ev.seq).or_default();
                    bucket.push(ev);
                }
                Err(TryRecvError::Empty) => {
                    break;
                }
                _ => {
                    error!("receive_events got Err from channel, error?");
                    self.status = ClientStatus::Error;
                    break;
                }
            }
        }
        //trace!("rx_flatten: {:?}", self.rx_flatten);
    }

    pub fn try_recv(&mut self) -> Option<Event> {
        match self.rx.try_recv() {
            Ok(ev) => {
                self.recycle_request_id(&ev);
                Some(ev)
            }
            Err(TryRecvError::Empty) => None,
            _ => {
                error!("async part is down");
                self.status = ClientStatus::Error;
                None
            }
        }
    }

    pub fn poll_one(&mut self, request_id: RequestId) -> Option<Event> {
        match self.rx_flatten.remove(&request_id) {
            Some(mut events) => {
                trace!("poll_one {events:?}");
                if events.is_empty() {
                    None
                } else {
                    if events.len() > 1 {
                        warn!("poll_one() actually dropped more events");
                    }
                    Some(events.swap_remove(events.len() - 1))
                }
            }
            None => None,
        }
    }

    pub fn send_request(&mut self, nrl: Nrl, kind: RequestKind) -> RequestId {
        let seq = self.next_request_id();
        let ev = Event {
            nrl,
            kind: EventKind::Request {
                kind,
            },
            seq,
        };
        if self.tx.send(ev).is_err() {
            self.status = ClientStatus::Error;
        }
        seq
    }

    pub fn send_reply(&mut self, nrl: Nrl, result: Result<ReplyKind, XpiError>, seq: RequestId) {
        let ev = Event {
            nrl,
            kind: EventKind::Reply {
                result,
            },
            seq,
        };
        if self.tx.send(ev).is_err() {
            self.status = ClientStatus::Error;
        }
    }

    pub fn seq_status(&self) -> &HashMap<RequestId, SeqStatus> {
        &self.seq_status
    }

    pub fn debug_report(&self) -> String {
        let mut s = String::new();
        for (id, status) in self.seq_status.iter() {
            s += &format!("{id:?}: {status}\n");
        }
        s
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
pub enum ClientStatus {
    #[default]
    Norminal,
    Warning,
    Error,
}

#[derive(Debug)]
pub enum SeqStatus {
    AwaitingReply { created: Instant },
    Done { since: Instant },
    Streaming { last_update: Instant },
}

impl Display for SeqStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SeqStatus::AwaitingReply { created } => write!(
                f,
                "AwaitingReply for: {}s",
                Instant::now().duration_since(*created).as_secs()
            ),
            SeqStatus::Done { since } => write!(
                f,
                "Done since: {}s",
                Instant::now().duration_since(*since).as_secs()
            ),
            SeqStatus::Streaming { last_update } => write!(
                f,
                "Streaming, last update: {}s",
                Instant::now().duration_since(*last_update).as_secs()
            ),
        }
    }
}

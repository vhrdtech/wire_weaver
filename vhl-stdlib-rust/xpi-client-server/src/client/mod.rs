pub mod error;
pub mod ws;

use error::Error;
use std::collections::HashMap;
use tokio::sync::mpsc::{
    error::TryRecvError, unbounded_channel, UnboundedReceiver, UnboundedSender,
};
use tracing::{error, trace, warn};
use xpi::client_server_owned::{Event, Protocol, RequestId};

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

    pub fn blocking_split<S: AsRef<str>>(&mut self, name: S) -> Result<Client, Error> {
        let (tx_router, rx_node) = unbounded_channel();

        let seq_subset = self.seq_subset;
        self.seq_subset += 1; // TODO: handle overflow
        self.tx_internal
            .send(InternalReq::AddInstance {
                seq_subset,
                tx: tx_router,
                name: name.as_ref().to_owned(),
            })
            .map_err(|_| Error::SplitFailed)?;
        match self.rx_internal.blocking_recv() {
            Some(InternalResp::InstanceCreated) => {}
            _ => {
                return Err(Error::SplitFailed);
            }
        }
        Ok(Client {
            seq_subset,
            seq: 0,
            tx: self.tx_events.clone(),
            rx: rx_node,
            rx_flatten: HashMap::new(),
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
}

impl Client {
    pub fn recycle_request_id(&mut self, _seq: RequestId) {
        todo!()
    }

    pub fn next_request_id(&mut self) -> RequestId {
        let rid = RequestId(((self.seq_subset as u32) << 24) + self.seq);
        self.seq = self.seq.wrapping_add(1); // TODO: recycle request ids
        rid
    }

    pub fn receive_events(&mut self) {
        loop {
            match self.rx.try_recv() {
                Ok(ev) => {
                    let bucket = self.rx_flatten.entry(ev.seq).or_default();
                    bucket.push(ev);
                }
                Err(TryRecvError::Empty) => {
                    break;
                }
                _ => {
                    error!("receive_events got Err from channel, error?");
                    break;
                }
            }
        }
        //trace!("rx_flatten: {:?}", self.rx_flatten);
    }

    pub fn try_recv(&mut self) -> Option<Event> {
        match self.rx.try_recv() {
            Ok(ev) => Some(ev),
            Err(TryRecvError::Empty) => None,
            _ => {
                error!("async part is down");
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

    pub fn send(&mut self, event: Event) -> Result<(), Error> {
        self.tx.send(event).map_err(|_| Error::QueueError)
    }
}

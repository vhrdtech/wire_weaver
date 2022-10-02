use std::fmt::{Debug, Display, Formatter};
use futures::channel::mpsc::Sender;
use xpi::owned::{Event, NodeId};
use crate::node::addressing::RemoteNodeAddr;

pub struct RemoteDescriptor {
    pub reachable: Vec<NodeId>,
    pub addr: RemoteNodeAddr,
    pub to_event_loop: Sender<Event>,
}

impl Debug for RemoteDescriptor {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl Display for RemoteDescriptor {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "RemoteDescriptor{{ {:?} at {:?} }}", self.reachable, self.addr)
    }
}
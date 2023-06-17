use crate::node::addressing::RemoteNodeAddr;
use futures::channel::mpsc::Sender;
use std::fmt::{Debug, Display, Formatter};
use xpi::node_owned::{Event, NodeId};

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
        write!(
            f,
            "RemoteDescriptor{{ {:?} at {:?} }}",
            self.reachable, self.addr
        )
    }
}

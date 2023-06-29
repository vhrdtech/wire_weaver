use futures::channel::mpsc::Sender;
use std::fmt::{Debug, Display, Formatter};
use xpi::client_server_owned::{Event, Protocol};

pub struct RemoteDescriptor {
    // pub reachable: Vec<NodeId>,
    pub protocol: Protocol,
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
            "RemoteDescriptor{{ at {:?} }}",
            self.protocol
        )
    }
}

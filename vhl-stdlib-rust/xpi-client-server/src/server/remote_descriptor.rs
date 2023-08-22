use crate::server::internal_event::InternalEventToEventLoop;
use futures::channel::mpsc::Sender;
use std::fmt::{Debug, Display, Formatter};
use xpi::client_server_owned::{AddressableEvent, Protocol};

pub struct RemoteDescriptor {
    // pub reachable: Vec<NodeId>,
    pub protocol: Protocol,
    pub to_event_loop: Sender<AddressableEvent>,
    pub to_event_loop_internal: Sender<InternalEventToEventLoop>,
}

impl Debug for RemoteDescriptor {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl Display for RemoteDescriptor {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "RemoteDescriptor{{ at {:?} }}", self.protocol)
    }
}

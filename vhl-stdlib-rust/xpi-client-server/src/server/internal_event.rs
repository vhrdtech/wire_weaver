use crate::filter::EventFilter;
use crate::server::control_event::NrlSpecificDispatcherHandle;
use futures::channel::mpsc::Sender;
use xpi::client_server_owned::{AddressableEvent, Nrl, Protocol};

use super::remote_descriptor::RemoteDescriptor;

#[derive(Debug)]
pub enum InternalEvent {
    // ConnectInstance(NodeId, Sender<AddressableEvent>),
    // DisconnectInstance(NodeId),
    ConnectRemote(RemoteDescriptor),
    DropRemote(Protocol),
    Filter(EventFilter, Sender<AddressableEvent>), // TODO: add timeout to remove if filter_one no longer waits for it
}

/// Goes directly to event loop of a particular client
#[derive(Debug)]
pub enum InternalEventToEventLoop {
    RegisterDispatcherForNrl(NrlSpecificDispatcherHandle), // DropAllRelatedTo(Protocol),
    DropDispatcherForNrl(Nrl),
}

use crate::filter::EventFilter;
use futures::channel::mpsc::{channel, Receiver, Sender};
use xpi::client_server_owned::{AddressableEvent, Nrl, Protocol};
use crate::server::NrlSpecificHandler;

use super::remote_descriptor::RemoteDescriptor;

#[derive(Debug)]
pub enum InternalEvent {
    // ConnectInstance(NodeId, Sender<AddressableEvent>),
    // DisconnectInstance(NodeId),
    ConnectRemote(RemoteDescriptor),
    DropRemote(Protocol),
    Filter(EventFilter, Sender<AddressableEvent>), // TODO: add timeout to remove if filter_one no longer waits for it
}

#[derive(Clone, Debug)]
pub struct DispatcherHandle {
    pub protocol: Protocol,
    pub nrl: Nrl,
    pub tx: Sender<AddressableEvent>,
}

impl DispatcherHandle {
    pub fn new(protocol: Protocol, nrl: Nrl) -> (Self, Receiver<AddressableEvent>) {
        let (tx, rx) = channel(64);
        (Self { protocol, nrl, tx }, rx)
    }
}

/// Goes directly to event loop of a particular client
#[derive(Debug)]
pub enum InternalEventToEventLoop {
    RegisterDispatcherForNrl(NrlSpecificHandler)
    // DropAllRelatedTo(Protocol),
}

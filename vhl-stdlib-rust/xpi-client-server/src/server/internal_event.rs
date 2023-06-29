use crate::{filter::EventFilter, remote::remote_descriptor::RemoteDescriptor};
use futures::channel::mpsc::Sender;
use xpi::client_server_owned::{Event, NodeId, Protocol};

#[derive(Debug)]
pub enum InternalEvent {
    ConnectInstance(NodeId, Sender<Event>),
    DisconnectInstance(NodeId),
    ConnectRemote(RemoteDescriptor),
    DropRemote(Protocol),
    Filter(EventFilter, Sender<Event>), // TODO: add timeout to remove if filter_one no longer waits for it
}

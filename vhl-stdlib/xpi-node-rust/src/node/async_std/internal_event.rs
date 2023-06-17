use crate::node::{addressing::RemoteNodeAddr, filter::EventFilter};
use crate::remote::remote_descriptor::RemoteDescriptor;
use futures::channel::mpsc::Sender;
use std::net::SocketAddr;
use xpi::node_owned::{Event, NodeId};

#[derive(Debug)]
pub enum InternalEvent {
    ConnectInstance(NodeId, Sender<Event>),
    DisconnectInstance(NodeId),
    ConnectRemote(RemoteDescriptor),
    DropRemote(RemoteNodeAddr),
    Filter(EventFilter, Sender<Event>), // TODO: add timeout to remove if filter_one no longer waits for it
}

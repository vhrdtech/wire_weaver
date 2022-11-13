use std::net::SocketAddr;
use xpi::owned::{Event, NodeId};
use futures::channel::mpsc::Sender;
use crate::node::filter::EventFilter;
use crate::remote::remote_descriptor::RemoteDescriptor;

#[derive(Debug)]
pub enum InternalEvent {
    ConnectInstance(NodeId, Sender<Event>),
    ConnectRemoteTcp(RemoteDescriptor),
    DropRemoteTcp(SocketAddr),
    FilterOne(EventFilter, Sender<Event>), // TODO: add timeout to remove if filter_one no longer waits for it
}

use core::fmt::{Debug, Formatter};
use crate::serdes::xpi_vlu4::error::FailReason;
use crate::xpi::addressing::{XpiGenericNodeSet, XpiGenericResourceSet};
use crate::xpi::broadcast::XpiGenericBroadcastKind;
use crate::xpi::event::{XpiGenericEvent, XpiGenericEventKind};
use crate::xpi::priority::XpiGenericPriority;
use crate::xpi::reply::XpiGenericReply;
use crate::xpi::request::{XpiGenericRequest, XpiGenericRequestKind};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerialUri {
    pub segments: Vec<SerialUriSegment>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SerialUriSegment {
    Serial { serial: u32 },
    SerialIndex { serial: u32, by: u32 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerialMultiUri {}

#[derive(Copy, Clone)]
pub struct Rate {}

#[derive(Copy, Clone)]
pub struct RequestId(pub u32);

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct NodeId(pub u32);

pub type XpiResourceSet = XpiGenericResourceSet<SerialUri, SerialMultiUri>;

/// Owned XpiRequest relying on allocators and std
/// See [XpiGenericRequest](crate::xpi::request::XpiGenericRequest) for detailed information.
pub type XpiRequest = XpiGenericRequest<
    SerialUri,
    SerialMultiUri,
    Vec<u8>,
    Vec<Vec<u8>>,
    Vec<Rate>,
    RequestId,
>;

/// See [XpiGenericRequestKind](crate::xpi::request::XpiGenericRequestKind) for detailed information.
pub type XpiRequestKind<'req> = XpiGenericRequestKind<
    Vec<u8>,
    Vec<Vec<u8>>,
    Vec<Rate>
>;

#[derive(Clone, Debug)]
pub struct ResourceInfo {}

pub type XpiReply = XpiGenericReply<
    SerialUri,
    SerialMultiUri,
    Vec<Vec<u8>>,
    Vec<Result<Vec<u8>, FailReason>>,
    Vec<Result<(), FailReason>>,
    Vec<Result<(), ResourceInfo>>,
    RequestId
>;

pub type XpiBroadcastKind = XpiGenericBroadcastKind<
    (),
    u32,
>;

#[derive(Clone, Debug)]
pub struct TraitSet {}

pub type NodeSet = XpiGenericNodeSet<NodeId, TraitSet>;

pub type Priority = XpiGenericPriority<u8>;

pub type XpiEvent = XpiGenericEvent<
    NodeId,
    TraitSet,
    XpiRequest,
    XpiReply,
    XpiBroadcastKind,
    (),
    Priority
>;

pub type XpiEventKind = XpiGenericEventKind<
    XpiRequest,
    XpiReply,
    XpiBroadcastKind,
    (),
>;

impl XpiEventKind {
    pub fn new_request(resource_set: XpiResourceSet, kind: XpiRequestKind, id: RequestId) -> Self {
        XpiEventKind::Request(XpiRequest {
            resource_set,
            kind,
            request_id: id,
        })
    }

    pub fn new_heartbeat(info: u32) -> Self {
        XpiGenericEventKind::Broadcast(XpiGenericBroadcastKind::Heartbeat(info))
    }
}

impl XpiEvent {
    pub fn new(source: NodeId, destination: NodeSet, kind: XpiEventKind, priority: Priority) -> Self {
        XpiEvent {
            source,
            destination,
            kind,
            priority,
        }
    }
}

impl Debug for XpiEvent {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "XpiEvent")
    }
}
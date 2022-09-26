use std::fmt::{Debug, Formatter};
use crate::xpi::broadcast::XpiGenericBroadcastKind;
use crate::xpi::event::{XpiGenericEvent, XpiGenericEventKind};
use super::{
    NodeId,
    TraitSet,
    XpiRequest,
    XpiReply,
    XpiBroadcastKind,
    Priority,
    NodeSet,
    XpiResourceSet,
    XpiRequestKind,
    RequestId
};

pub type XpiEvent = XpiGenericEvent<
    NodeId,
    TraitSet,
    XpiRequest,
    XpiReply,
    XpiBroadcastKind,
    (),
    Priority
>;

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

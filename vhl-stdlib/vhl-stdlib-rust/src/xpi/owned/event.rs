use std::fmt::{Debug, Formatter};
use crate::serdes::xpi_vlu4::event::XpiEventVlu4;
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

pub type XpiEventOwned = XpiGenericEvent<
    NodeId,
    TraitSet,
    XpiRequest,
    XpiReply,
    XpiBroadcastKind,
    (),
    Priority
>;

impl XpiEventOwned {
    pub fn new(source: NodeId, destination: NodeSet, kind: XpiEventKind, priority: Priority) -> Self {
        XpiEventOwned {
            source,
            destination,
            kind,
            priority,
        }
    }

    fn try_into_vlu4<'ev>(&self) -> Result<XpiEventVlu4<'ev>, ()> {
        todo!()
    }
}

impl Debug for XpiEventOwned {
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

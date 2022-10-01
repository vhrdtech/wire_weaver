use crate::broadcast::XpiGenericBroadcastKind;
use crate::event::{XpiGenericEvent, XpiGenericEventKind};
use crate::owned::convert_error::ConvertError;
use crate::owned::node_id::NodeId;
use crate::owned::node_set::NodeSet;
use crate::owned::request_id::RequestId;
use crate::owned::resource_set::{ResourceSet, ResourceSetConvertXwfd};
use crate::owned::trait_set::TraitSet;
use crate::xwfd;
use crate::xwfd::xwfd_info::XwfdInfo;
use std::fmt::{Debug, Formatter};
use vhl_stdlib::serdes::bit_buf::BitBufMut;
use vhl_stdlib::serdes::traits::SerializeBits;
use vhl_stdlib::serdes::{bit_buf, NibbleBufMut};

use super::{BroadcastKind, Priority, Reply, Request, RequestKind};

pub type Event = XpiGenericEvent<NodeId, TraitSet, Request, Reply, BroadcastKind, (), Priority>;

impl Event {
    pub fn new(source: NodeId, destination: NodeSet, kind: EventKind, priority: Priority) -> Self {
        Event {
            source,
            destination,
            kind,
            priority,
        }
    }
}

impl Event {
    pub fn ser_xwfd(&self, nwr: &mut NibbleBufMut) -> Result<(), ConvertError> {
        // Some(_) if resource set is Uri only & it's a request or response
        let mut resource_set: Option<ResourceSetConvertXwfd> = None;
        nwr.as_bit_buf::<_, ConvertError>(|bwr| {
            bwr.put_up_to_8(3, 0b000)?; // unused 31:29
            let priority: xwfd::Priority = self.priority.try_into()?;
            bwr.put(&priority)?; // bits 28:26
            bwr.put(&self.kind)?; // bits 25:24 - event kind
            bwr.put_bit(true)?; // bit 23 - is_xwfd_or_bigger
            let node_id: xwfd::NodeId = self.source.try_into()?;
            bwr.put(&node_id)?; // bits 22:16
            self.destination.ser_header_xwfd(bwr)?; // bits 15:7 - destination node or node set
            resource_set = self.kind.ser_header_xwfd(bwr)?;
            Ok(())
        })?;
        nwr.put(&XwfdInfo::FormatIsXwfd)?;
        self.destination.ser_body_xwfd(nwr)?;
        self.kind.ser_body_xwfd(nwr, resource_set)?;
        Ok(())
    }
}

impl Debug for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "XpiEvent")
    }
}

pub type EventKind = XpiGenericEventKind<Request, Reply, BroadcastKind, ()>;

impl EventKind {
    pub fn new_request(resource_set: ResourceSet, kind: RequestKind, id: RequestId) -> Self {
        EventKind::Request(Request {
            resource_set,
            kind,
            request_id: id,
        })
    }

    pub fn new_heartbeat(info: u32) -> Self {
        EventKind::Broadcast(XpiGenericBroadcastKind::Heartbeat(info))
    }

    pub(crate) fn ser_header_xwfd(
        &self,
        bwr: &mut BitBufMut,
    ) -> Result<Option<ResourceSetConvertXwfd>, ConvertError> {
        match &self {
            EventKind::Request(req) => {
                // bits 6:4 - discriminant of ResourceSet+Uri
                let rs = req.resource_set.ser_header_xwfd(bwr)?;
                req.kind.ser_header_xwfd(bwr)?; // bits 3:0 - request kind
                Ok(Some(rs))
            }
            EventKind::Reply(rep) => {
                // bwr.put(&rep.resource_set)?; // bits 6:4 - discriminant of ResourceSet+Uri
                let rs = rep.resource_set.ser_header_xwfd(bwr)?;
                rep.kind.ser_header_xwfd(bwr)?; // bits 3:0 - reply kind
                Ok(Some(rs))
            }
            EventKind::Broadcast(_) => todo!(),
            EventKind::Forward(_) => todo!(),
        }
    }

    pub(crate) fn ser_body_xwfd(
        &self,
        nwr: &mut NibbleBufMut,
        resource_set: Option<ResourceSetConvertXwfd>,
    ) -> Result<(), ConvertError> {
        match &self {
            EventKind::Request(req) => {
                resource_set.expect("").ser_body_xwfd(nwr)?;
                req.kind.ser_body_xwfd(nwr)?; // bits 3:0 - request kind
                Ok(())
            }
            EventKind::Reply(rep) => {
                resource_set.expect("").ser_body_xwfd(nwr)?;
                rep.kind.ser_body_xwfd(nwr)?; // bits 3:0 - reply kind
                Ok(())
            }
            EventKind::Broadcast(_) => todo!(),
            EventKind::Forward(_) => todo!(),
        }
    }
}

impl SerializeBits for EventKind {
    type Error = bit_buf::Error;

    fn ser_bits(&self, bwr: &mut BitBufMut) -> Result<(), Self::Error> {
        let bits = match self {
            EventKind::Request(_) => 0b11,
            EventKind::Reply(_) => 0b10,
            EventKind::Broadcast(_) => 0b00,
            EventKind::Forward(_) => 0b01,
        };
        bwr.put_up_to_8(2, bits)?;
        Ok(())
    }
}

use std::fmt::{Debug, Formatter};
use vhl_stdlib_nostd::serdes::bit_buf::BitBufMut;
use vhl_stdlib_nostd::serdes::{bit_buf, NibbleBufMut, SerDesSize};
use vhl_stdlib_nostd::serdes::traits::SerializeBits;
use crate::broadcast::XpiGenericBroadcastKind;
use crate::event::{XpiGenericEvent, XpiGenericEventKind};
use crate::owned::convert_error::ConvertError;
use crate::owned::node_id::NodeId;
use crate::owned::node_set::NodeSet;
use crate::owned::request_id::RequestId;
use crate::owned::resource_set::ResourceSet;
use crate::owned::trait_set::TraitSet;

use super::{
    Priority,
    BroadcastKind,
    Reply,
    Request,
    RequestKind,
};

pub type Event = XpiGenericEvent<
    NodeId,
    TraitSet,
    Request,
    Reply,
    BroadcastKind,
    (),
    Priority
>;

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
    pub fn ser_xwfd(&self, _nwr: &mut NibbleBufMut) -> Result<(), ConvertError> {
        // nwr.as_bit_buf::<_, ConvertError>(|bwr| {
        //     bwr.put_up_to_8(3, 0b000)?; // unused 31:29
        //     bwr.put(&self.priority.try_into()?)?; // bits 28:26
        //     bwr.put(&self.kind)?; // bits 25:24 - event kind
        //     bwr.put_bit(false)?; // bit 23 - is_bit_wf
        //     bwr.put(&self.source.try_into()?)?; // bits 22:16
        //     self.destination.ser_header_xwfd(bwr)?; // bits 15:7 - destination node or node set
        //     match &self.kind {
        //         XpiGenericEventKind::Request(req) => {
        //             // bits 6:4 - discriminant of ResourceSet+Uri
        //
        //             bwr.put(&req.kind)?; // bits 3:0 - request kind
        //         }
        //         XpiGenericEventKind::Reply(rep) => {
        //             bwr.put(&rep.resource_set)?; // bits 6:4 - discriminant of ResourceSet+Uri
        //             bwr.put(&rep.kind)?; // bits 3:0 - reply kind
        //         }
        //         XpiGenericEventKind::Broadcast(_) => todo!(),
        //         XpiGenericEventKind::Forward(_) => todo!(),
        //     }
        //     Ok(())
        // })?;
        // nwr.put(&XwfdInfo::FormatIsXwfd)?;

        Ok(())
    }

    fn len_nibbles(&self) -> SerDesSize {
        todo!()
    }
}

impl Debug for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "XpiEvent")
    }
}

pub type EventKind = XpiGenericEventKind<
    Request,
    Reply,
    BroadcastKind,
    (),
>;

impl EventKind {
    pub fn new_request(resource_set: ResourceSet, kind: RequestKind, id: RequestId) -> Self {
        EventKind::Request(Request {
            resource_set,
            kind,
            request_id: id,
        })
    }

    pub fn new_heartbeat(info: u32) -> Self {
        XpiGenericEventKind::Broadcast(XpiGenericBroadcastKind::Heartbeat(info))
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
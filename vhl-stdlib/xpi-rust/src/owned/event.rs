use std::fmt::{Debug, Formatter};
use vhl_stdlib_nostd::serdes::bit_buf::BitBufMut;
use vhl_stdlib_nostd::serdes::{bit_buf, NibbleBufMut, SerDesSize};
use vhl_stdlib_nostd::serdes::traits::SerializeBits;
use crate::broadcast::XpiGenericBroadcastKind;
use crate::event::{XpiGenericEvent, XpiGenericEventKind};
use crate::owned::error::ConvertError;

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
    RequestId,
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
}

impl XpiEventOwned {
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

impl SerializeBits for XpiEventKind {
    type Error = bit_buf::Error;

    fn ser_bits(&self, bwr: &mut BitBufMut) -> Result<(), Self::Error> {
        let bits = match self {
            XpiEventKind::Request(_) => 0b11,
            XpiEventKind::Reply(_) => 0b10,
            XpiEventKind::Broadcast(_) => 0b00,
            XpiEventKind::Forward(_) => 0b01,
        };
        bwr.put_up_to_8(2, bits)?;
        Ok(())
    }
}
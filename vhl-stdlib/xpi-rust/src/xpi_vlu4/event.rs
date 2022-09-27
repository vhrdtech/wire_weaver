use vhl_stdlib_nostd::serdes::{DeserializeCoupledBitsVlu4, DeserializeVlu4, NibbleBuf};
use vhl_stdlib_nostd::serdes::vlu4::TraitSet;
use crate::event::{XpiGenericEvent, XpiGenericEventKind};
use crate::xpi_vlu4::{addressing::{XpiResourceSet, NodeSet}, request::{XpiRequestVlu4, XpiRequestKindVlu4}, reply::{XpiReplyVlu4}, broadcast::XpiBroadcastKind, error::{XpiVlu4Error}, priority::Priority, NodeId};
use crate::xpi_vlu4::addressing::RequestId;
use crate::xpi_vlu4::reply::XpiReplyKindVlu4;

pub type XpiEventVlu4<'ev> = XpiGenericEvent<
    NodeId,
    TraitSet<'ev>,
    XpiRequestVlu4<'ev>,
    XpiReplyVlu4<'ev>,
    XpiBroadcastKind,
    (),
    Priority
>;

pub type XpiEventKindVlu4<'ev> = XpiGenericEventKind<
    XpiRequestVlu4<'ev>,
    XpiReplyVlu4<'ev>,
    XpiBroadcastKind,
    ()
>;

impl<'i> DeserializeVlu4<'i> for XpiEventVlu4<'i> {
    type Error = XpiVlu4Error;

    fn des_vlu4<'di>(rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        // get first 32 bits as BitBuf
        let mut bits_rdr = rdr.get_bit_buf(8)?;
        let _absent_31_29 = bits_rdr.get_up_to_8(3);

        // bits 28:26
        let priority: Priority = bits_rdr.des_bits()?;

        // bit 25:24
        let kind1 = bits_rdr.get_bit()?;
        let kind0 = bits_rdr.get_bit()?;

        // UAVCAN reserved bit 23, discard if 0 (UAVCAN discards if 1).
        let reserved_23 = bits_rdr.get_bit()?;
        if !reserved_23 {
            return Err(XpiVlu4Error::ReservedDiscard);
        }

        // bits: 22:16
        let source: NodeId = bits_rdr.des_bits()?;

        // bits: 15:7 + variable nibbles if not NodeSet::Unicast
        let destination = NodeSet::des_coupled_bits_vlu4(&mut bits_rdr, rdr)?;

        // bits 6:4 + 1/2/3/4 nibbles for Uri::OnePart4/TwoPart44/ThreePart* or variable otherwise
        let resource_set = XpiResourceSet::des_coupled_bits_vlu4(&mut bits_rdr, rdr)?;

        let kind = match (kind1, kind0) {
            (false, false) => {
                // Broadcast
                return Err(XpiVlu4Error::Unimplemented);
                // XpiEventKind::Broadcast(XpiBroadcastKind::DiscoverNodes)
            }
            (false, true) => {
                // Forward
                return Err(XpiVlu4Error::Unimplemented);
            }
            (true, false) => {
                // Reply, kind in bits 3:0
                let kind = XpiReplyKindVlu4::des_coupled_bits_vlu4(&mut bits_rdr, rdr)?;
                // tail byte should be at byte boundary, if not 4b padding is added
                rdr.align_to_byte()?;
                let request_id: RequestId = rdr.des_vlu4()?;
                XpiEventKindVlu4::Reply(XpiReplyVlu4 { resource_set, kind, request_id })
            }
            (true, true) => {
                // Request, kind in bits 3:0
                let kind = XpiRequestKindVlu4::des_coupled_bits_vlu4(&mut bits_rdr, rdr)?;
                // tail byte should be at byte boundary, if not 4b padding is added
                rdr.align_to_byte()?;
                let request_id: RequestId = rdr.des_vlu4()?;
                XpiEventKindVlu4::Request(XpiRequestVlu4 { resource_set, kind, request_id })
            }
        };

        Ok(XpiEventVlu4 {
            source,
            destination,
            kind,
            priority,
        })
    }
}
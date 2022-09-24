use crate::serdes::{DeserializeVlu4, NibbleBuf};
use crate::serdes::traits::DeserializeCoupledBitsVlu4;
use crate::serdes::xpi_vlu4::addressing::{NodeSet, RequestId, XpiResourceSet};
use crate::serdes::xpi_vlu4::broadcast::XpiBroadcastKind;
use crate::serdes::xpi_vlu4::error::XpiVlu4Error;
use crate::serdes::xpi_vlu4::NodeId;
use crate::serdes::xpi_vlu4::priority::Priority;
use crate::serdes::xpi_vlu4::reply::{XpiReply, XpiReplyKind};
use crate::serdes::xpi_vlu4::request::{XpiRequest, XpiRequestKind};
use crate::xpi::event::{XpiGenericEvent, XpiGenericEventKind};

pub type XpiEvent<'ev> = XpiGenericEvent<
    NodeId,
    NodeSet<'ev>,
    XpiRequest<'ev>,
    XpiReply<'ev>,
    XpiBroadcastKind,
    (),
    Priority
>;

pub type XpiEventKind<'ev> = XpiGenericEventKind<
    XpiRequest<'ev>,
    XpiReply<'ev>,
    XpiBroadcastKind,
    ()
>;

impl<'i> DeserializeVlu4<'i> for XpiEvent<'i> {
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
                let kind = XpiReplyKind::des_coupled_bits_vlu4(&mut bits_rdr, rdr)?;
                // tail byte should be at byte boundary, if not 4b padding is added
                rdr.align_to_byte()?;
                let request_id: RequestId = rdr.des_vlu4()?;
                XpiEventKind::Reply(XpiReply { resource_set, kind, request_id })
            }
            (true, true) => {
                // Request, kind in bits 3:0
                let kind = XpiRequestKind::des_coupled_bits_vlu4(&mut bits_rdr, rdr)?;
                // tail byte should be at byte boundary, if not 4b padding is added
                rdr.align_to_byte()?;
                let request_id: RequestId = rdr.des_vlu4()?;
                XpiEventKind::Request(XpiRequest { resource_set, kind, request_id })
            }
        };

        Ok(XpiEvent {
            source,
            destination,
            kind,
            priority,
        })
    }
}
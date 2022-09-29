use vhl_stdlib_nostd::serdes::{DeserializeCoupledBitsVlu4, DeserializeVlu4, NibbleBuf};
use vhl_stdlib_nostd::serdes::vlu4::TraitSet;
use crate::event::{XpiGenericEvent, XpiGenericEventKind};
use crate::xwfd::xwfd_info::XwfdInfo;
use crate::xwfd::node_set::NodeSet;
use super::{
    broadcast::BroadcastKind, error::XwfdError, NodeId,
    Priority, Reply,
    ReplyKind, RequestId, ResourceSet,
    XpiRequestKindVlu4,
    XpiRequestVlu4,
};

pub type Event<'ev> = XpiGenericEvent<
    NodeId,
    TraitSet<'ev>,
    XpiRequestVlu4<'ev>,
    Reply<'ev>,
    BroadcastKind,
    (),
    Priority
>;

pub type EventKind<'ev> = XpiGenericEventKind<
    XpiRequestVlu4<'ev>,
    Reply<'ev>,
    BroadcastKind,
    ()
>;

impl<'i> DeserializeVlu4<'i> for Event<'i> {
    type Error = XwfdError;

    fn des_vlu4<'di>(rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        // get first 32 bits as BitBuf
        let mut bits_rdr = rdr.get_bit_buf(8)?;
        let _absent_31_29 = bits_rdr.get_up_to_8(3);

        // bits 28:26
        let priority: Priority = bits_rdr.des_bits()?;

        // bit 25:24
        let kind1 = bits_rdr.get_bit()?;
        let kind0 = bits_rdr.get_bit()?;

        // bit 23: is_xwfd_or_bigger
        let is_xwfd_or_bigger = bits_rdr.get_bit()?;
        if !is_xwfd_or_bigger {
            return Err(XwfdError::ReservedDiscard);
        }
        let format_info: XwfdInfo = rdr.des_vlu4()?;
        if format_info != XwfdInfo::FormatIsXwfd {
            return Err(XwfdError::WrongFormat);
        }

        // bits: 22:16
        let source: NodeId = bits_rdr.des_bits()?;

        // bits: 15:7 + variable nibbles if not NodeSet::Unicast
        let destination = NodeSet::des_coupled_bits_vlu4(&mut bits_rdr, rdr)?;

        // bits 6:4 + 1/2/3/4 nibbles for Uri::OnePart4/TwoPart44/ThreePart* or variable otherwise
        let resource_set = ResourceSet::des_coupled_bits_vlu4(&mut bits_rdr, rdr)?;

        let kind = match (kind1, kind0) {
            (false, false) => {
                // Broadcast
                return Err(XwfdError::Unimplemented);
                // XpiEventKind::Broadcast(XpiBroadcastKind::DiscoverNodes)
            }
            (false, true) => {
                // Forward
                return Err(XwfdError::Unimplemented);
            }
            (true, false) => {
                // Reply, kind in bits 3:0
                let kind = ReplyKind::des_coupled_bits_vlu4(&mut bits_rdr, rdr)?;
                // tail byte should be at byte boundary, if not 4b padding is added
                rdr.align_to_byte()?;
                let request_id: RequestId = rdr.des_vlu4()?;
                EventKind::Reply(Reply { resource_set, kind, request_id })
            }
            (true, true) => {
                // Request, kind in bits 3:0
                let kind = XpiRequestKindVlu4::des_coupled_bits_vlu4(&mut bits_rdr, rdr)?;
                // tail byte should be at byte boundary, if not 4b padding is added
                rdr.align_to_byte()?;
                let request_id: RequestId = rdr.des_vlu4()?;
                EventKind::Request(XpiRequestVlu4 { resource_set, kind, request_id })
            }
        };

        Ok(Event {
            source,
            destination,
            kind,
            priority,
        })
    }
}

#[cfg(test)]
mod test {
    use hex_literal::hex;
    use vhl_stdlib_nostd::serdes::NibbleBuf;
    use crate::xwfd::XwfdError;
    use super::Event;

    #[test]
    fn des_is_xwdf_or_bigger_false() {
        let buf = hex!("02 55 10 90 04 50");
        let mut nrd = NibbleBuf::new_all(&buf);
        let r: Result<Event, XwfdError> = nrd.des_vlu4();

        matches!(r, Err(XwfdError::WrongFormat));
    }

    #[test]
    fn des_is_xwdf_info_other_format() {
        let buf = hex!("02 d5 10 90 84 50");
        let mut nrd = NibbleBuf::new_all(&buf);
        let r: Result<Event, XwfdError> = nrd.des_vlu4();

        matches!(r, Err(XwfdError::WrongFormat));
    }
}
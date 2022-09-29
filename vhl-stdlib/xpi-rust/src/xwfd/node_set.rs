use std::fmt::{Display, Formatter, Result as FmtResult};
use vhl_stdlib_nostd::serdes::{bit_buf, BitBuf, BitBufMut, DeserializeCoupledBitsVlu4, nibble_buf, NibbleBuf, NibbleBufMut, SerDesSize, SerializeBits, SerializeVlu4};
use vhl_stdlib_nostd::serdes::vlu4::TraitSet;
use crate::node_set::XpiGenericNodeSet;
use crate::xwfd::{NodeId, XwfdError};

pub type NodeSet<'i> = XpiGenericNodeSet<NodeId, TraitSet<'i>>;

impl<'i> DeserializeCoupledBitsVlu4<'i> for NodeSet<'i> {
    type Error = XwfdError;

    fn des_coupled_bits_vlu4<'di>(
        bits_rdr: &'di mut BitBuf<'i>,
        _vlu4_rdr: &'di mut NibbleBuf<'i>,
    ) -> Result<Self, Self::Error> {
        let kind = bits_rdr.get_up_to_8(2)?;
        match kind {
            0b00 => Ok(NodeSet::Unicast(bits_rdr.des_bits()?)),
            0b01 => Err(XwfdError::Unimplemented),
            0b10 => Err(XwfdError::Unimplemented),
            0b11 => Ok(NodeSet::Broadcast),
            _ => Err(XwfdError::InternalError),
        }
    }
}

impl<'i> SerializeBits for NodeSet<'i> {
    type Error = bit_buf::Error;

    fn ser_bits(&self, wgr: &mut BitBufMut) -> Result<(), Self::Error> {
        // must write 9 bits
        match self {
            NodeSet::Unicast(id) => {
                wgr.put_up_to_8(2, 0b00)?;
                wgr.put(id)?;
            }
            NodeSet::UnicastTraits { .. } => {
                wgr.put_up_to_8(2, 0b01)?;
                todo!()
            }
            NodeSet::Multicast { .. } => {
                wgr.put_up_to_8(2, 0b10)?;
                todo!()
            }
            NodeSet::Broadcast => {
                wgr.put_up_to_8(2, 0b11)?;
                wgr.put_up_to_8(7, 0)?;
            }
        };
        Ok(())
    }
}

impl<'i> SerializeVlu4 for NodeSet<'i> {
    type Error = nibble_buf::Error;

    fn ser_vlu4(&self, _wgr: &mut NibbleBufMut) -> Result<(), Self::Error> {
        match self {
            NodeSet::Unicast(_) | NodeSet::Broadcast => {
                // Unicast was already serialized into header, no need to add anything
                return Ok(());
            }
            NodeSet::UnicastTraits { .. } => {
                todo!()
            }
            NodeSet::Multicast { .. } => {
                todo!()
            }
        }
    }

    fn len_nibbles(&self) -> SerDesSize {
        SerDesSize::Sized(0)
    }
}

impl<'i> Display for NodeSet<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            NodeSet::Unicast(node_id) => write!(f, "{}", node_id),
            NodeSet::UnicastTraits {
                destination,
                traits,
            } => write!(f, "{}{}", destination, traits),
            NodeSet::Multicast { .. } => write!(f, "M_impl"),
            NodeSet::Broadcast => write!(f, "*")
        }
    }
}

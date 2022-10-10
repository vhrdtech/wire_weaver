use core::fmt::{Display, Formatter, Result as FmtResult};
use vhl_stdlib::discrete::U9;
use vhl_stdlib::serdes::{
    BitBuf,
    DeserializeCoupledBitsVlu4,
    nibble_buf,
    NibbleBuf,
    NibbleBufMut,
    SerDesSize,
    SerializeVlu4,
    vlu4::TraitSet,
};
use crate::error::XpiError;
use crate::node_set::XpiGenericNodeSet;
use crate::xwfd::NodeId;

pub type NodeSet<'i> = XpiGenericNodeSet<NodeId, TraitSet<'i>>;

impl<'i> DeserializeCoupledBitsVlu4<'i> for NodeSet<'i> {
    type Error = XpiError;

    fn des_coupled_bits_vlu4<'di>(
        brd: &'di mut BitBuf<'i>,
        _nrd: &'di mut NibbleBuf<'i>,
    ) -> Result<Self, Self::Error> {
        let kind = brd.get_up_to_8(2)?;
        match kind {
            0b00 => Ok(NodeSet::Unicast(brd.des_bits()?)),
            0b01 => Err(XpiError::Unimplemented),
            0b10 => Err(XpiError::Unimplemented),
            0b11 => {
                let original_source = brd.des_bits()?;
                Ok(NodeSet::Broadcast { original_source })
            },
            _ => Err(XpiError::Internal),
        }
    }
}

impl<'i> NodeSet<'i> {
    pub fn ser_header(&self) -> U9 {
        let bits = match self {
            NodeSet::Unicast(id) => {
                0b00_000_0000 | (id.inner() as u16)
            }
            NodeSet::UnicastTraits { .. } => {
                todo!()
            }
            NodeSet::Multicast { .. } => {
                todo!()
            }
            NodeSet::Broadcast { original_source } => {
                0b11_000_0000 | (original_source.inner() as u16)
            }
        };
        unsafe { U9::new_unchecked(bits) }
    }
}

// impl<'i> SerializeBits for NodeSet<'i> {
//     type Error = bit_buf::Error;
//
//     fn ser_bits(&self, nwr: &mut BitBufMut) -> Result<(), Self::Error> {
//         // must write 9 bits
//         match self {
//             NodeSet::Unicast(id) => {
//                 nwr.put_up_to_8(2, 0b00)?;
//                 nwr.put(id)?;
//             }
//             NodeSet::UnicastTraits { .. } => {
//                 nwr.put_up_to_8(2, 0b01)?;
//                 todo!()
//             }
//             NodeSet::Multicast { .. } => {
//                 nwr.put_up_to_8(2, 0b10)?;
//                 todo!()
//             }
//             NodeSet::Broadcast { original_source } => {
//                 nwr.put_up_to_8(2, 0b11)?;
//                 nwr.put(original_source)?;
//             }
//         };
//         Ok(())
//     }
// }

impl<'i> SerializeVlu4 for NodeSet<'i> {
    type Error = nibble_buf::Error;

    fn ser_vlu4(&self, _wgr: &mut NibbleBufMut) -> Result<(), Self::Error> {
        match self {
            NodeSet::Unicast(_) | NodeSet::Broadcast { .. } => {
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
            NodeSet::Broadcast { .. } => write!(f, "*")
        }
    }
}

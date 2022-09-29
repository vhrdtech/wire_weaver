use vhl_stdlib_nostd::{
    discrete::max_bound_number,
    serdes::{
        bit_buf,
        BitBuf,
        BitBufMut,
        nibble_buf,
        NibbleBuf,
        NibbleBufMut,
        SerDesSize,
        DeserializeCoupledBitsVlu4, SerializeBits, SerializeVlu4, DeserializeBits, DeserializeVlu4,
        vlu4::TraitSet,
    },
};
use super::{
    SerialUri, SerialMultiUri,
    MultiUriFlatIter,
};
use core::fmt::{Display, Formatter, Result as FmtResult};
use crate::addressing::{XpiGenericNodeSet, XpiGenericResourceSet};
use crate::xwfd::error::XwfdError;

max_bound_number!(NodeId, 7, u8, 127, "N:{}", put_up_to_8, get_up_to_8);
impl<'i> DeserializeVlu4<'i> for NodeId {
    type Error = XwfdError;

    fn des_vlu4<'di>(rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        Ok(NodeId::new(rdr.get_u8()?).ok_or_else(|| XwfdError::NodeIdAbove127)?)
    }
}

// Each outgoing request must be marked with an increasing number in order to distinguish
// requests of the same kind and map responses.
// Might be narrowed down to less bits. Detect an overflow when old request(s) was still unanswered.
// Should pause in that case or cancel all old requests. Overflow is ignored for subscriptions.
max_bound_number!(RequestId, u8, 31, "Req:{}");
impl<'i> DeserializeVlu4<'i> for RequestId {
    type Error = XwfdError;

    fn des_vlu4<'di>(rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        let tail_byte = rdr.get_u8()?;
        let request_id = tail_byte & 0b0001_1111;
        Ok(RequestId(request_id & 0b0001_1111))
    }
}

impl SerializeVlu4 for RequestId {
    type Error = XwfdError;

    fn ser_vlu4(&self, wgr: &mut NibbleBufMut) -> Result<(), Self::Error> {
        // since request id is a part of a tail byte, put padding before it to align
        wgr.align_to_byte()?;
        wgr.put_u8(self.inner())?;
        Ok(())
    }

    fn len_nibbles(&self) -> SerDesSize {
        SerDesSize::SizedAligned(2, 1)
    }
}

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

/// Vlu4 implementation of XpiGenericResourceSet.
/// See documentation for [XpiGenericResourceSet](crate::xpi::addressing::XpiGenericResourceSet)
pub type ResourceSet<'i> = XpiGenericResourceSet<SerialUri<'i>, SerialMultiUri<'i>>;

impl<'i> ResourceSet<'i> {
    pub fn flat_iter(&'i self) -> MultiUriFlatIter<'i> {
        match self {
            ResourceSet::Uri(uri) => MultiUriFlatIter::OneUri(Some(uri.iter())),
            ResourceSet::MultiUri(multi_uri) => multi_uri.flat_iter(),
        }
    }
}

impl<'i> SerializeBits for ResourceSet<'i> {
    type Error = bit_buf::Error;

    fn ser_bits(&self, wgr: &mut BitBufMut) -> Result<(), Self::Error> {
        let kind = match self {
            ResourceSet::Uri(uri) => match uri {
                SerialUri::OnePart4(_) => 0,
                SerialUri::TwoPart44(_, _) => 1,
                SerialUri::ThreePart444(_, _, _) => 2,
                SerialUri::ThreePart633(_, _, _) => 3,
                SerialUri::ThreePart664(_, _, _) => 4,
                SerialUri::MultiPart(_) => 5,
            },
            ResourceSet::MultiUri(_) => 6,
        };
        wgr.put_up_to_8(3, kind)
    }
}

impl<'i> SerializeVlu4 for ResourceSet<'i> {
    type Error = XwfdError;

    fn ser_vlu4(&self, wgr: &mut NibbleBufMut) -> Result<(), Self::Error> {
        match self {
            ResourceSet::Uri(uri) => wgr.put(uri),
            ResourceSet::MultiUri(multi_uri) => wgr.put(multi_uri),
        }
    }

    fn len_nibbles(&self) -> SerDesSize {
        match self {
            ResourceSet::Uri(uri) => uri.len_nibbles(),
            ResourceSet::MultiUri(multi_uri) => multi_uri.len_nibbles(),
        }
    }
}

impl<'i> DeserializeCoupledBitsVlu4<'i> for ResourceSet<'i> {
    type Error = XwfdError;

    fn des_coupled_bits_vlu4<'di>(
        bits_rdr: &'di mut BitBuf<'i>,
        vlu4_rdr: &'di mut NibbleBuf<'i>,
    ) -> Result<Self, Self::Error> {
        let uri_type = bits_rdr.get_up_to_8(3)?;
        match uri_type {
            0 => Ok(ResourceSet::Uri(SerialUri::OnePart4(vlu4_rdr.des_vlu4()?))),
            1 => Ok(ResourceSet::Uri(SerialUri::TwoPart44(
                vlu4_rdr.des_vlu4()?,
                vlu4_rdr.des_vlu4()?,
            ))),
            2 => Ok(ResourceSet::Uri(SerialUri::ThreePart444(
                vlu4_rdr.des_vlu4()?,
                vlu4_rdr.des_vlu4()?,
                vlu4_rdr.des_vlu4()?,
            ))),
            3 => {
                let mut bits = vlu4_rdr.get_bit_buf(3)?;
                Ok(ResourceSet::Uri(SerialUri::ThreePart633(
                    bits.des_bits()?,
                    bits.des_bits()?,
                    bits.des_bits()?,
                )))
            }
            4 => {
                let mut bits = vlu4_rdr.get_bit_buf(4)?;
                Ok(ResourceSet::Uri(SerialUri::ThreePart664(
                    bits.des_bits()?,
                    bits.des_bits()?,
                    bits.des_bits()?,
                )))
            }
            5 => Ok(ResourceSet::Uri(vlu4_rdr.des_vlu4()?)),
            6 => Ok(ResourceSet::MultiUri(vlu4_rdr.des_vlu4()?)),
            7 => Err(XwfdError::ReservedDiscard),
            _ => Err(XwfdError::InternalError),
        }
    }
}

impl<'i> Display for ResourceSet<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            ResourceSet::Uri(uri) => write!(f, "{}", uri),
            ResourceSet::MultiUri(multi_uri) => write!(f, "{}", multi_uri),
        }
    }
}

use vhl_stdlib_nostd::{
    serdes::{
        bit_buf,
        BitBuf,
        BitBufMut,
        DeserializeCoupledBitsVlu4,
        nibble_buf,
        NibbleBuf, NibbleBufMut, SerDesSize, SerializeBits, SerializeVlu4,
        vlu4::TraitSet,
    },
};
use super::{
    MultiUriFlatIter,
    SerialMultiUri,
    SerialUri,
};
use core::fmt::{Display, Formatter, Result as FmtResult};
use crate::addressing::XpiGenericResourceSet;
use crate::xwfd::error::XwfdError;

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

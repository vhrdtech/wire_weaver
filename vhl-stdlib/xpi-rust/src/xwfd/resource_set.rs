use vhl_stdlib::{
    serdes::{
        BitBuf,
        DeserializeCoupledBitsVlu4,
        NibbleBuf, NibbleBufMut, SerDesSize, SerializeVlu4,
        vlu4::{Vlu32, Vlu4Vec, Vlu4VecIter},
    },
};
use super::{
    MultiUriFlatIter,
    SerialMultiUri,
    SerialUri,
};
use core::fmt::{Display, Formatter, Result as FmtResult};
use vhl_stdlib::discrete::U3;
use vhl_stdlib::serdes::nibble_buf;
use crate::resource_set::XpiGenericResourceSet;
use crate::xwfd::error::XwfdError;

/// Vlu4 implementation of XpiGenericResourceSet.
/// See documentation for [XpiGenericResourceSet](crate::xpi::addressing::XpiGenericResourceSet)
pub type ResourceSet<'i> = XpiGenericResourceSet<SerialUri<Vlu4VecIter<'i, Vlu32>>, SerialMultiUri<'i>>;

impl<'i> ResourceSet<'i> {
    pub fn flat_iter(&'i self) -> MultiUriFlatIter<'i> {
        match self {
            ResourceSet::Uri(uri) => MultiUriFlatIter::OneUri(Some(uri.iter())),
            ResourceSet::MultiUri(multi_uri) => multi_uri.flat_iter(),
        }
    }

    pub fn ser_header(&self) -> U3 {
        match self {
            ResourceSet::Uri(uri) => unsafe { U3::new_unchecked(uri.discriminant() as u8) },
            ResourceSet::MultiUri(_) => unsafe { U3::new_unchecked(6) }
        }
    }
}
//
// impl<'i> SerializeBits for ResourceSet<'i> {
//     type Error = bit_buf::Error;
//
//     fn ser_bits(&self, wgr: &mut BitBufMut) -> Result<(), Self::Error> {
//         let kind = match self {
//             ResourceSet::Uri(uri) => uri.discriminant() as u8,
//             ResourceSet::MultiUri(_) => 6,
//         };
//         wgr.put_up_to_8(3, kind)
//     }
// }

impl<'i> SerializeVlu4 for ResourceSet<'i> {
    type Error = nibble_buf::Error;

    fn ser_vlu4(&self, wgr: &mut NibbleBufMut) -> Result<(), Self::Error> {
        match self {
            ResourceSet::Uri(uri) => wgr.put(uri)?,
            ResourceSet::MultiUri(multi_uri) => wgr.put(multi_uri)?,
        }
        Ok(())
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
            5 => {
                let arr: Vlu4Vec<Vlu32> = vlu4_rdr.des_vlu4()?;
                Ok(ResourceSet::Uri(SerialUri::MultiPart(arr.into_iter())))
            },
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

use crate::error::XpiError;
use vhl_stdlib::serdes::{
    nibble_buf, DeserializeVlu4, NibbleBuf, NibbleBufMut, SerDesSize, SerializeVlu4,
};

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum XwfdInfo {
    OtherFormat,
    FormatIsXwfd,
}

impl SerializeVlu4 for XwfdInfo {
    type Error = nibble_buf::Error;

    fn ser_vlu4(&self, nwr: &mut NibbleBufMut) -> Result<(), Self::Error> {
        let nib = match self {
            XwfdInfo::OtherFormat => 0b1000,
            XwfdInfo::FormatIsXwfd => 0b0000,
        };
        nwr.put_nibble(nib)?;
        Ok(())
    }

    fn len_nibbles(&self) -> SerDesSize {
        SerDesSize::Sized(1)
    }
}

impl<'i> DeserializeVlu4<'i> for XwfdInfo {
    type Error = XpiError;

    fn des_vlu4<'di>(nrd: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        match nrd.get_nibble()? {
            0b1000 => Ok(XwfdInfo::OtherFormat),
            0b0000 => Ok(XwfdInfo::FormatIsXwfd),
            _ => Err(XpiError::WrongFormat),
        }
    }
}

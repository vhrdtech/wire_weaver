use crate::serdes::{DeserializeVlu4, NibbleBuf, NibbleBufMut};
use crate::serdes::traits::SerializeVlu4;
use crate::serdes::xpi_vlu4::error::XpiVlu4Error;

/// One u8 slice, aligned to byte boundary.
///
/// 4 bit padding is inserted and skipped if needed before the slices data start.
#[derive(Copy, Clone, Debug)]
pub struct Vlu4Slice<'i> {
    pub slice: &'i [u8]
}

impl<'i> DeserializeVlu4<'i> for Vlu4Slice<'i> {
    type Error = XpiVlu4Error;

    fn des_vlu4<'di>(rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        let len = rdr.get_vlu4_u32()?;
        if !rdr.is_at_byte_boundary() {
            let _padding = rdr.get_nibble()?;
        }
        Ok(Vlu4Slice {
            slice: rdr.get_slice(len as usize)?
        })
    }
}

impl<'i> SerializeVlu4 for Vlu4Slice<'i> {
    type Error = XpiVlu4Error;

    fn ser_vlu4(&self, wgr: &mut NibbleBufMut) -> Result<(), Self::Error> {
        wgr.put_vlu4_u32(self.slice.len() as u32)?;
        if !wgr.is_at_byte_boundary() {
            wgr.put_nibble(0)?;
        }
        wgr.put_slice(self.slice)?;
        Ok(())
    }
}
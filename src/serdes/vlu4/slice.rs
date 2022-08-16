use crate::serdes::{DeserializeVlu4, NibbleBuf};

/// One u8 slice, aligned to byte boundary.
///
/// 4 bit padding is inserted and skipped if needed before the slices data start.
#[derive(Copy, Clone, Debug)]
pub struct Vlu4Slice<'i> {
    pub slice: &'i [u8]
}

impl<'i> DeserializeVlu4<'i> for Vlu4Slice<'i> {
    type Error = crate::serdes::nibble_buf::Error;

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
use vhl_stdlib::max_bound_number;
use vhl_stdlib::serdes::{
    DeserializeVlu4, NibbleBuf,
    SerializeVlu4, NibbleBufMut, SerDesSize,
};
use crate::xwfd::error::XwfdError;

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
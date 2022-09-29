use vhl_stdlib_nostd::max_bound_number;
use vhl_stdlib_nostd::serdes::{DeserializeVlu4, NibbleBuf, bit_buf, SerializeBits, DeserializeBits};
use crate::xwfd::error::XwfdError;

max_bound_number!(NodeId, 7, u8, 127, "N:{}", put_up_to_8, get_up_to_8);
impl<'i> DeserializeVlu4<'i> for NodeId {
    type Error = XwfdError;

    fn des_vlu4<'di>(rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        Ok(NodeId::new(rdr.get_u8()?).ok_or_else(|| XwfdError::NodeIdAbove127)?)
    }
}
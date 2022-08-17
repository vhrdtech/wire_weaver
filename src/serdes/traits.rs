use crate::serdes::{NibbleBuf, NibbleBufMut, BitBuf};
use crate::serdes::bit_buf::BitBufMut;

pub trait SerializeVlu4 {
    type Error;

    fn ser_vlu4(&self, wgr: &mut NibbleBufMut) -> Result<(), Self::Error>;
}

/// Deserialize trait implemented by all types that support deserializing from buffer of nibbles.
/// 'i lifetime refers to the byte slice used when creating NibbleBuf.
/// 'di lifetime is for mutably borrowing NibbleBuf only while deserializing,
///     deserialized objects can hold non mutable links to the original buffer ('i).
pub trait DeserializeVlu4<'i>: Sized {
    type Error;

    fn des_vlu4<'di>(rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error>;
}

/// Serialize trait that is implemented by all types that support serializing into bit buffers.
pub trait SerializeBits {
    type Error;

    fn ser_bits(&self, wgr: &mut BitBufMut) -> Result<(), Self::Error>;
}

/// Deserialize trait implemented by all types that support deserializing from buffer of bits.
/// 'i lifetime refers to the byte slice used when creating BitBuf.
/// 'di lifetime is for mutably borrowing BitBuf only while deserializing,
///     deserialized objects can hold non mutable links to the original buffer ('i).
pub trait DeserializeBits<'i>: Sized {
    type Error;

    fn des_bits<'di>(rdr: &'di mut BitBuf<'i>) -> Result<Self, Self::Error>;
}
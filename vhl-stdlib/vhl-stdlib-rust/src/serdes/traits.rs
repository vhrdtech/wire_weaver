use crate::serdes::bit_buf::BitBufMut;
use crate::serdes::buf::{Buf, BufMut};
use crate::serdes::{BitBuf, NibbleBuf, NibbleBufMut, SerDesSize};

pub trait SerializeBytes {
    type Error;

    fn ser_bytes(&self, wr: &mut BufMut) -> Result<(), Self::Error>;

    /// Size of the object serialized in bytes
    fn len_bytes(&self) -> SerDesSize;
}

/// Deserialize trait implemented by all types that support deserializing from buffer of bytes.
/// 'i lifetime refers to the byte slice used when creating NibbleBuf.
/// 'di lifetime is for mutably borrowing NibbleBuf only while deserializing,
///     deserialized objects can hold non mutable links to the original buffer ('i).
pub trait DeserializeBytes<'i>: Sized {
    type Error;

    fn des_bytes<'di>(rd: &'di mut Buf<'i>) -> Result<Self, Self::Error>;
}

pub trait SerializeVlu4 {
    type Error;

    fn ser_vlu4(&self, nwr: &mut NibbleBufMut) -> Result<(), Self::Error>;

    /// Size of the object serialized in nibbles
    fn len_nibbles(&self) -> SerDesSize;
}

/// Deserialize trait implemented by all types that support deserializing from buffer of nibbles.
/// 'i lifetime refers to the byte slice used when creating NibbleBuf.
/// 'di lifetime is for mutably borrowing NibbleBuf only while deserializing,
///     deserialized objects can hold non mutable links to the original buffer ('i).
pub trait DeserializeVlu4<'i>: Sized {
    type Error;

    fn des_vlu4<'di>(nrd: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error>;
}

/// Serialize trait that is implemented by all types that support serializing into bit buffers.
pub trait SerializeBits {
    type Error;

    fn ser_bits(&self, bwr: &mut BitBufMut) -> Result<(), Self::Error>;
}

/// Deserialize trait implemented by all types that support deserializing from buffer of bits.
/// 'i lifetime refers to the byte slice used when creating BitBuf.
/// 'di lifetime is for mutably borrowing BitBuf only while deserializing,
///     deserialized objects can hold non mutable links to the original buffer ('i).
pub trait DeserializeBits<'i>: Sized {
    type Error;

    fn des_bits<'di>(bwr: &'di mut BitBuf<'i>) -> Result<Self, Self::Error>;
}

/// Deserialize trait implemented by all types that can be deserialized from 2 places at once.
///
/// For example one reader can be used to read from a packet header, while
/// the second one to read associated data from the same packet's data portion.
pub trait DeserializeCoupledBitsVlu4<'i>: Sized {
    type Error;

    fn des_coupled_bits_vlu4<'di>(
        brd: &'di mut BitBuf<'i>,
        nrd: &'di mut NibbleBuf<'i>,
    ) -> Result<Self, Self::Error>;
}

// pub trait DeserializeCoupledReadersTupleVlu4Bits<'i, T> {
//     type Error;
//
//     fn des_coupled_vlu4_bits<'di>(&mut self) -> Result<T, Self::Error>;
// }
//
// impl<'i, T> DeserializeCoupledReadersTupleVlu4Bits<'i, T> for (&&mut BitBuf<'i>, &&mut NibbleBuf<'i>)
//     where T: DeserializeCoupledVlu4Bits<'i>
// {
//     type Error = <T as DeserializeCoupledVlu4Bits<'i>>::Error;
//
//     fn des_coupled_vlu4_bits<'di>(&mut self) -> Result<T, Self::Error> {
//         T::des_coupled_vlu4_bits(*self.0, *self.1)
//     }
// }

pub trait SerializableError: Sized {
    fn error_code(&self) -> u32;
    fn from_error_code(code: u32) -> Option<Self>;
}

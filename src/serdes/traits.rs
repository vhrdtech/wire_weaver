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

/// Deserialize trait implemented by all types that can be deserialized from 2 places at once.
///
/// For example one reader can be used to read from a packet header, while
/// the second one to read associated data from the same packet's data portion.
/// same packet.
pub trait DeserializeCoupledBitsVlu4<'i>: Sized {
    type Error;

    fn des_coupled_bits_vlu4<'di>(
        bits_rdr: &'di mut BitBuf<'i>,
        vlu4_rdr: &'di mut NibbleBuf<'i>,
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
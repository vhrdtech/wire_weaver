use vhl_stdlib_nostd::serdes::{bit_buf};
use vhl_stdlib_nostd::serdes::bit_buf::BitBufMut;
use vhl_stdlib_nostd::serdes::traits::SerializeBits;
use crate::priority::XpiGenericPriority;

pub type Priority = XpiGenericPriority<u8>;

impl SerializeBits for Priority {
    type Error = bit_buf::Error;

    fn ser_bits(&self, bwr: &mut BitBufMut) -> Result<(), Self::Error> {
        todo!()
    }
}
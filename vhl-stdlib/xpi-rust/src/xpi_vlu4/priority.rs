use vhl_stdlib_nostd::discrete::U2Sp1;
use vhl_stdlib_nostd::serdes::bit_buf::BitBufMut;
use vhl_stdlib_nostd::serdes::traits::SerializeBits;
use vhl_stdlib_nostd::serdes::{bit_buf, BitBuf, DeserializeBits};
use core::fmt::{Display, Formatter};
use crate::priority::XpiGenericPriority;

pub type Priority = XpiGenericPriority<U2Sp1>;

impl<'i> DeserializeBits<'i> for Priority {
    type Error = bit_buf::Error;

    fn des_bits<'di>(rdr: &'di mut BitBuf<'i>) -> Result<Self, Self::Error> {
        let is_lossless = rdr.get_bit()?;
        if is_lossless {
            Ok(Priority::Lossless(rdr.des_bits()?))
        } else {
            Ok(Priority::Lossy(rdr.des_bits()?))
        }
    }
}

impl SerializeBits for Priority {
    type Error = bit_buf::Error;

    fn ser_bits(&self, wgr: &mut BitBufMut) -> Result<(), Self::Error> {
        let (is_lossless, level) = match self {
            Priority::Lossy(level) => (false, level),
            Priority::Lossless(level) => (true, level),
        };
        wgr.put_bit(is_lossless)?;
        wgr.put(level)
    }
}

impl Display for Priority {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            Priority::Lossy(level) => write!(f, "L{}", level.to_u8()),
            Priority::Lossless(level) => write!(f, "R{}", level.to_u8()),
        }
    }
}

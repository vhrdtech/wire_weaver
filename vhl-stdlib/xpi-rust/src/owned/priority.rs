use vhl_stdlib_nostd::discrete::U2;
use vhl_stdlib_nostd::serdes::{bit_buf};
use vhl_stdlib_nostd::serdes::bit_buf::BitBufMut;
use vhl_stdlib_nostd::serdes::traits::SerializeBits;
use crate::owned::error::ConvertError;
use crate::priority::XpiGenericPriority;
use crate::xwfd;

pub type Priority = XpiGenericPriority<u8>;

impl TryInto<xwfd::Priority> for Priority {
    type Error = ConvertError;

    fn try_into(self) -> Result<xwfd::Priority, Self::Error> {
        match self {
            Priority::Lossy(level) => {
                if level <= 3 {
                    Ok(xwfd::Priority::Lossy(unsafe { U2::new_unchecked(level as u8) }))
                } else {
                    Err(ConvertError::PriorityTruncate)
                }
            }
            Priority::Lossless(level) => {
                if level <= 3 {
                    Ok(xwfd::Priority::Lossless(unsafe { U2::new_unchecked(level as u8) }))
                } else {
                    Err(ConvertError::PriorityTruncate)
                }
            }
        }
    }
}

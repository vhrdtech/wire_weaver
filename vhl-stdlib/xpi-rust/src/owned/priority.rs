use crate::owned::convert_error::ConvertError;
use crate::priority::XpiGenericPriority;
use crate::xwfd;
use std::fmt::{Display, Formatter};
use vhl_stdlib::discrete::U2;

pub type Priority = XpiGenericPriority<u8>;

impl TryInto<xwfd::Priority> for Priority {
    type Error = ConvertError;

    fn try_into(self) -> Result<xwfd::Priority, Self::Error> {
        match self {
            Priority::Lossy(level) => {
                if level <= 3 {
                    Ok(xwfd::Priority::Lossy(unsafe {
                        U2::new_unchecked(level as u8)
                    }))
                } else {
                    Err(ConvertError::PriorityTruncate)
                }
            }
            Priority::Lossless(level) => {
                if level <= 3 {
                    Ok(xwfd::Priority::Lossless(unsafe {
                        U2::new_unchecked(level as u8)
                    }))
                } else {
                    Err(ConvertError::PriorityTruncate)
                }
            }
        }
    }
}

impl From<xwfd::Priority> for Priority {
    fn from(priority: xwfd::Priority) -> Self {
        match priority {
            xwfd::Priority::Lossy(lvl) => Priority::Lossy(lvl.inner() as u8),
            xwfd::Priority::Lossless(lvl) => Priority::Lossless(lvl.inner() as u8),
        }
    }
}

impl Display for Priority {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Priority::Lossy(lvl) => write!(f, "Lo{}", lvl),
            Priority::Lossless(lvl) => write!(f, "Re{}", lvl),
        }
    }
}

use core::fmt::{Display, Formatter};
use crate::discrete::U2Sp1;
use crate::serdes::{BitBuf, DeserializeBits};
use crate::serdes::xpi_vlu4::error::XpiVlu4Error;


/// Priority selection: lossy or lossless (to an extent).
/// Truly lossless mode is not achievable, for example if connection is physically lost mid-transfer,
/// or memory is exceeded.
///
/// Higher priority in either mode means higher chance of successfully transferring a message.
/// If channels is wide enough, all messages will go through unaffected.
///
/// Some form of fair queueing must be implemented not to starve lossy channels by lossless ones.
/// Or several underlying channels may be used to separate the two. Up to the Link to decide on
/// implementation.
///
/// Some form of rate shaping should be implemented to be able to work with different channel speeds.
/// Rates can be changed in real time, limiting property observing or streams bandwidth.
/// TCP algorithms for congestion control may be applied here?
/// Alternatively discrete event simulation may be attempted to prove lossless properties.
/// Knowing streaming rates and precise size of various messages can help with that.
///
/// If loss occurs in lossy mode, it is silently ignored.
/// If loss occurs in lossless mode, it is flagged as an error.
///
/// Priority may be mapped into fewer levels by the underlying Link? (needed for constrained channels)
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Priority {
    Lossy(U2Sp1), // numbers must be u<7, +1> (range 1..=128) or natural to avoid confusions
    Lossless(U2Sp1),
}

impl<'i> DeserializeBits<'i> for Priority {
    type Error = XpiVlu4Error;

    fn des_bits<'di>(rdr: &'di mut BitBuf<'i>) -> Result<Self, Self::Error> {
        let is_lossless = rdr.get_bit()?;
        if is_lossless {
            Ok(Priority::Lossless(rdr.des_bits()?))
        } else {
            Ok(Priority::Lossy(rdr.des_bits()?))
        }
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
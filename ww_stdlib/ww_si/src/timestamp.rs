//! Host side of [Second] as a device timestamp: from wrapped values back to the full device time.

use crate::ops::scale_i128;
use crate::{Prefix, Second};
use ww_numeric::NumericValue;

/// Turns timestamps a device sends in fixed-width storage (a `U32` of µs wraps every 71.6 minutes) back into the full
/// device time since its clock started, as a `U64` in the timestamp's own prefix.
///
/// Each timestamp is taken as the value closest to the previous one that has the same low bits, so it is right as
/// long as consecutive timestamps are less than half a wrap period apart (35.8 minutes for a `U32` of µs). The first
/// one is taken as the closest to an anchor, a full-width reading of the same clock (`ww_timebase::Clock::now()`,
/// [new](Self::new)), or without an anchor ([default](Self::default)) as is: the earliest time it can be, right when
/// the clock started counting or less than one wrap period after. Re-anchor or [restart](Self::restart) after a long
/// pause or when the device's clock restarted (a reboot).
///
/// The prefix and storage may change from one timestamp to the next (a device that autoranges, or sends `U16` ms
/// here and `U32` µs there): the previous time is carried over into the new prefix. Values that don't wrap (`U64`,
/// `UVlq64`, signed integers) are passed through, floats are refused.
///
/// Use one per stream, or per sequence of timestamps that come in order: timestamps of different streams can be
/// interleaved out of order.
///
/// ```
/// use ww_si::{Second, Unwrapper};
///
/// // the device's clock read in full: 5000 s and a bit, in µs
/// let now = Second::micro(5_000_000_123u64);
/// let mut unwrapper = Unwrapper::new(now).unwrap();
/// // a timestamp of the same moment as U32 µs: 5_000_000_123 mod 2^32
/// let wrapped = Second::micro(705_032_827u32);
/// let full = unwrapper.unwrap(wrapped).unwrap();
/// assert_eq!(full, Second::micro(5_000_000_123u64));
///
/// // no anchor: counts from the first timestamp on, across wraps
/// let mut unwrapper = Unwrapper::default();
/// assert_eq!(unwrapper.unwrap(Second::micro(u32::MAX)).unwrap(), Second::micro(u32::MAX as u64));
/// assert_eq!(unwrapper.unwrap(Second::micro(5u32)).unwrap(), Second::micro((1u64 << 32) + 5));
/// ```
#[derive(Clone, Debug, Default)]
pub struct Unwrapper {
    /// Last full time, in units of `10^exp` seconds, `(time, exp)`; `None` before the first timestamp without anchor.
    reference: Option<(i128, i8)>,
}

/// Why a timestamp could not be unwrapped.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UnwrapError {
    /// The value is a float, or `UN` / `IN`: timestamps are integers.
    NotAnInteger,
    /// The full value does not fit in a `u64` of the timestamp's prefix, or the prefixes are too far apart.
    Overflow,
}

impl core::fmt::Display for UnwrapError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            UnwrapError::NotAnInteger => write!(f, "timestamp is not an integer"),
            UnwrapError::Overflow => write!(f, "timestamp does not fit in u64"),
        }
    }
}

impl core::error::Error for UnwrapError {}

impl Unwrapper {
    /// Anchored on `now`, a full-width reading of the device's clock (any prefix, integer storage).
    pub fn new(now: Second) -> Result<Self, UnwrapError> {
        let reference = now.value.to_i128().ok_or(UnwrapError::NotAnInteger)?;
        Ok(Unwrapper {
            reference: Some((reference, now.prefix.into())),
        })
    }

    /// Anchor again, e.g. after reconnecting or a pause longer than half a wrap period.
    pub fn anchor(&mut self, now: Second) -> Result<(), UnwrapError> {
        *self = Self::new(now)?;
        Ok(())
    }

    /// Forget the times so far: the next timestamp is taken as if it were the first one without an anchor. For a
    /// device whose clock restarted.
    pub fn restart(&mut self) {
        self.reference = None;
    }

    /// The last full time, from the anchor or the last unwrapped timestamp, in its prefix; `None` before the first
    /// timestamp without an anchor.
    pub fn last(&self) -> Option<Second> {
        let (reference, exp) = self.reference?;
        let prefix = Prefix::try_from(exp).unwrap_or(Prefix::Unit);
        Some(Second::new(prefix, NumericValue::U64(reference as u64)))
    }

    /// The full device time of `timestamp`, as a `U64` in `timestamp`'s prefix.
    pub fn unwrap(&mut self, timestamp: Second) -> Result<Second, UnwrapError> {
        let value = timestamp.value.to_i128().ok_or(UnwrapError::NotAnInteger)?;
        let exp: i8 = timestamp.prefix.into();
        let full = match (timestamp.value.unsigned_bits(), self.reference) {
            (Some(bits), Some((reference, ref_exp))) if bits < 64 => {
                let reference =
                    scale_i128(reference, ref_exp - exp).ok_or(UnwrapError::Overflow)?;
                closest(reference, value, 1i128 << bits)
            }
            _ => value,
        };
        let full_u64 = u64::try_from(full).map_err(|_| UnwrapError::Overflow)?;
        self.reference = Some((full, exp));
        Ok(Second::new(timestamp.prefix, NumericValue::U64(full_u64)))
    }
}

/// The non-negative value congruent to `low` modulo `modulus` closest to `reference`.
fn closest(reference: i128, low: i128, modulus: i128) -> i128 {
    let mut candidate = reference - reference.rem_euclid(modulus) + low;
    if candidate - reference > modulus / 2 {
        candidate -= modulus;
    } else if reference - candidate > modulus / 2 {
        candidate += modulus;
    }
    // time since the clock started is never negative
    if candidate < 0 {
        candidate += modulus;
    }
    candidate
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    fn us32(x: u64) -> Second {
        Second::micro((x % (1 << 32)) as u32)
    }

    #[test]
    fn follows_wraps() {
        let start = 3 * (1u64 << 32) - 1000;
        let mut unwrapper = Unwrapper::new(Second::micro(start)).unwrap();
        // every 10 minutes for 5 hours: several wraps
        for i in 0..30u64 {
            let t = start + i * 600_000_000;
            assert_eq!(
                unwrapper.unwrap(us32(t)).unwrap().value,
                NumericValue::U64(t)
            );
        }
    }

    #[test]
    fn before_and_after_anchor() {
        let now = 10 * (1u64 << 32) + 5;
        let mut unwrapper = Unwrapper::new(Second::micro(now)).unwrap();
        // a timestamp from just before the anchor, across the wrap
        let earlier = now - 100;
        assert_eq!(
            unwrapper.unwrap(us32(earlier)).unwrap(),
            Second::micro(earlier)
        );
        let later = now + 1_000_000;
        assert_eq!(unwrapper.unwrap(us32(later)).unwrap(), Second::micro(later));
    }

    #[test]
    fn anchor_in_another_prefix() {
        // anchor in ns, timestamps as u16 ms (wraps every 65.5 s)
        let now_ns = 1_000_000_123_456_789u64; // ~11.6 days
        let mut unwrapper = Unwrapper::new(Second::nano(now_ns)).unwrap();
        let t_ms = now_ns / 1_000_000 + 20;
        let full = unwrapper
            .unwrap(Second::milli((t_ms % 65536) as u16))
            .unwrap();
        assert_eq!(full.prefix, Prefix::Milli);
        assert_eq!(full.value, NumericValue::U64(t_ms));
        assert_eq!(unwrapper.last(), Some(Second::milli(t_ms)));
    }

    #[test]
    fn early_after_start_is_not_negative() {
        let mut unwrapper = Unwrapper::new(Second::micro(5u64)).unwrap();
        let t = (1u64 << 32) - 3;
        // can't be from before the clock started: it is t, not -3
        assert_eq!(unwrapper.unwrap(us32(t)).unwrap(), Second::micro(t));
    }

    #[test]
    fn full_width_passes_through() {
        let mut unwrapper = Unwrapper::new(Second::micro(5u64)).unwrap();
        let t = Second::micro(u64::MAX);
        assert_eq!(unwrapper.unwrap(t).unwrap(), t);
        assert_eq!(
            unwrapper.unwrap(Second::from_f32(1.0)),
            Err(UnwrapError::NotAnInteger)
        );
    }

    #[test]
    fn without_anchor_from_the_first_timestamp() {
        let mut unwrapper = Unwrapper::default();
        assert_eq!(unwrapper.last(), None);
        let start = (1u64 << 32) - 1000;
        // every 10 minutes for 5 hours, as U32 µs: several wraps
        for i in 0..30u64 {
            let t = start + i * 600_000_000;
            assert_eq!(unwrapper.unwrap(us32(t)).unwrap(), Second::micro(t));
        }
        // a little back is not a wrap
        let last = start + 29 * 600_000_000;
        assert_eq!(
            unwrapper.unwrap(us32(last - 100)).unwrap(),
            Second::micro(last - 100)
        );
    }

    #[test]
    fn u16_ms_wraps() {
        let mut unwrapper = Unwrapper::default();
        // every 20 s for 10 minutes: U16 ms wraps every 65.5 s
        for i in 0..30u64 {
            let t = 1_000 + i * 20_000;
            let full = unwrapper.unwrap(Second::milli((t % 65536) as u16)).unwrap();
            assert_eq!(full, Second::milli(t));
            assert_eq!(full.value, NumericValue::U64(t));
        }
    }

    #[test]
    fn prefix_and_storage_change_on_the_way() {
        let mut unwrapper = Unwrapper::default();
        // U16 ms past its first wrap, then U32 µs, then U64 ns, then U16 ms again
        let t_ms = 70_000u64;
        assert_eq!(
            unwrapper.unwrap(Second::milli(65535u16)).unwrap(),
            Second::milli(65535u64)
        );
        assert_eq!(
            unwrapper
                .unwrap(Second::milli((t_ms % 65536) as u16))
                .unwrap(),
            Second::milli(t_ms)
        );
        let t_us = t_ms * 1000 + 40;
        assert_eq!(
            unwrapper.unwrap(Second::micro(t_us as u32)).unwrap(),
            Second::micro(t_us)
        );
        let t_ns = t_us * 1000 + 7;
        assert_eq!(
            unwrapper.unwrap(Second::nano(t_ns)).unwrap(),
            Second::nano(t_ns)
        );
        let t_ms = t_ms + 3;
        assert_eq!(
            unwrapper
                .unwrap(Second::milli((t_ms % 65536) as u16))
                .unwrap(),
            Second::milli(t_ms)
        );
    }

    #[test]
    fn restart_forgets() {
        let mut unwrapper = Unwrapper::default();
        unwrapper.unwrap(us32(u32::MAX as u64)).unwrap();
        assert_eq!(
            unwrapper.unwrap(us32(5)).unwrap(),
            Second::micro((1u64 << 32) + 5)
        );
        // the device rebooted: its clock counts from 0 again
        unwrapper.restart();
        assert_eq!(unwrapper.unwrap(us32(5)).unwrap(), Second::micro(5u64));
    }
}

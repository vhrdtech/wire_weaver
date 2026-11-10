//! [TimeUnwrapper]: a [TimePoint] that wraps on the device back into its full 64-bit point on the host.

use crate::timed::TimePoint;
use core::marker::PhantomData;

/// Turns the points a device sends in narrow storage (a [DeviceMicros](crate::origins::DeviceMicros)`<u32>` wraps
/// every 71.6 minutes) back into the full point, [TimePoint::Full] (a `DeviceMicros<u64>`).
///
/// Each point is taken as the one closest to the previous one with the same low bits, so it is right as long as
/// consecutive points are less than half a wrap apart (35.8 minutes for `u32` µs, 32.7 s for `u16` ms). The first
/// one is taken as the closest to an anchor, a full reading of the same clock ([new](Self::new)), or without an anchor
/// ([default](Self::default)) as is: the earliest it can be. Re-anchor or [restart](Self::restart) after a long pause
/// or when the device's clock restarted (a reboot). A 64-bit storage passes through.
///
/// Use one per stream, or per sequence of points that come in order: points of different streams can be
/// interleaved out of order. The step and storage are the type's, so they can't change from one point to the next
/// (a device whose resolution changes at run time sends `ww_si::Second`, see [Unwrapper](crate::Unwrapper)).
///
/// ```
/// use ww_si::{TimeUnwrapper, origins::DeviceMicros};
///
/// // the device's clock read in full: 5000 s and a bit, in µs
/// let now = DeviceMicros::<u64>::from_raw(5_000_000_123);
/// let mut unwrapper = TimeUnwrapper::<DeviceMicros<u32>>::new(now);
/// // a point of the same moment as u32 µs: 5_000_000_123 mod 2^32
/// let full = unwrapper.unwrap(DeviceMicros::from_raw(705_032_827)).unwrap();
/// assert_eq!(full, now);
///
/// // no anchor: counts from the first point on, across wraps
/// let mut unwrapper = TimeUnwrapper::<DeviceMicros<u32>>::default();
/// assert_eq!(unwrapper.unwrap(DeviceMicros::from_raw(u32::MAX)).unwrap().raw(), u32::MAX as u64);
/// assert_eq!(unwrapper.unwrap(DeviceMicros::from_raw(5)).unwrap().raw(), (1 << 32) + 5);
/// ```
pub struct TimeUnwrapper<Tm> {
    /// The last full point's number; `None` before the first point without an anchor.
    reference: Option<i128>,
    _tm: PhantomData<fn() -> Tm>,
}

impl<Tm> Clone for TimeUnwrapper<Tm> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Tm> Copy for TimeUnwrapper<Tm> {}

impl<Tm> core::fmt::Debug for TimeUnwrapper<Tm> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TimeUnwrapper")
            .field("reference", &self.reference)
            .finish()
    }
}

impl<Tm> Default for TimeUnwrapper<Tm> {
    fn default() -> Self {
        TimeUnwrapper {
            reference: None,
            _tm: PhantomData,
        }
    }
}

/// The full point does not fit [TimePoint::Full]: past the end of a `u64` of steps, or before the scale's zero.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TimeOverflow;

impl core::fmt::Display for TimeOverflow {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "time point does not fit in 64 bits")
    }
}

impl core::error::Error for TimeOverflow {}

impl<Tm: TimePoint> TimeUnwrapper<Tm> {
    /// Anchored on `now`, a full reading of the device's clock.
    pub fn new(now: Tm::Full) -> Self {
        TimeUnwrapper {
            reference: Some(now.raw_int()),
            _tm: PhantomData,
        }
    }

    /// Anchor again, e.g. after reconnecting or a pause longer than half a wrap.
    pub fn anchor(&mut self, now: Tm::Full) {
        *self = Self::new(now);
    }

    /// Forget the points so far: the next one is taken as if it were the first one without an anchor. For a device
    /// whose clock restarted.
    pub fn restart(&mut self) {
        self.reference = None;
    }

    /// The last full point, from the anchor or the last unwrapped point; `None` before the first point without an
    /// anchor.
    pub fn last(&self) -> Option<Tm::Full> {
        self.reference
            .map(<Tm::Full as TimePoint>::from_int_wrapping)
    }

    /// The full point of `t`.
    pub fn unwrap(&mut self, t: Tm) -> Result<Tm::Full, TimeOverflow> {
        let value = t.raw_int();
        let full = match self.reference {
            Some(reference) if Tm::BITS < 64 => {
                let modulus = 1i128 << Tm::BITS;
                let candidate = closest(reference, value.rem_euclid(modulus), modulus);
                // before the scale's zero only where the full storage can hold it (an i64 of UTC, never a device's
                // time since boot)
                if candidate < 0 && !fits::<Tm::Full>(candidate) {
                    candidate + modulus
                } else {
                    candidate
                }
            }
            _ => value,
        };
        if !fits::<Tm::Full>(full) {
            return Err(TimeOverflow);
        }
        self.reference = Some(full);
        Ok(<Tm::Full as TimePoint>::from_int_wrapping(full))
    }
}

/// Whether `x` is a number of `T`.
fn fits<T: TimePoint>(x: i128) -> bool {
    T::from_int_wrapping(x).raw_int() == x
}

/// The value congruent to `low` modulo `modulus` closest to `reference`.
pub(crate) fn closest(reference: i128, low: i128, modulus: i128) -> i128 {
    let mut candidate = reference - reference.rem_euclid(modulus) + low;
    if candidate - reference > modulus / 2 {
        candidate -= modulus;
    } else if reference - candidate > modulus / 2 {
        candidate += modulus;
    }
    candidate
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use crate::origins::{DeviceMicros, DeviceMillis, UnixMicros};

    fn us32(x: u64) -> DeviceMicros<u32> {
        DeviceMicros::from_raw((x % (1 << 32)) as u32)
    }

    #[test]
    fn follows_wraps() {
        let start = 3 * (1u64 << 32) - 1000;
        let mut unwrapper = TimeUnwrapper::new(DeviceMicros::<u64>::from_raw(start));
        // every 10 minutes for 5 hours: several wraps
        for i in 0..30u64 {
            let t = start + i * 600_000_000;
            assert_eq!(unwrapper.unwrap(us32(t)).unwrap().raw(), t);
        }
        assert_eq!(unwrapper.last().unwrap().raw(), start + 29 * 600_000_000);
    }

    #[test]
    fn before_and_after_anchor() {
        let now = 10 * (1u64 << 32) + 5;
        let mut unwrapper = TimeUnwrapper::new(DeviceMicros::<u64>::from_raw(now));
        assert_eq!(unwrapper.unwrap(us32(now - 100)).unwrap().raw(), now - 100);
        assert_eq!(
            unwrapper.unwrap(us32(now + 1_000_000)).unwrap().raw(),
            now + 1_000_000
        );
    }

    #[test]
    fn early_after_boot_is_not_before_it() {
        let mut unwrapper = TimeUnwrapper::new(DeviceMicros::<u64>::from_raw(5));
        let t = (1u64 << 32) - 3;
        // can't be from before the device booted: it is t, not -3
        assert_eq!(unwrapper.unwrap(us32(t)).unwrap().raw(), t);
    }

    #[test]
    fn signed_scale_goes_below_zero() {
        // an i32 of ms of UTC around the Unix epoch: the full point may be before it
        type UtcMs32 = crate::Point<crate::origins::UnixEpoch, crate::Millis<i32>>;
        let mut unwrapper = TimeUnwrapper::<UtcMs32>::new(crate::Point::from_raw(3i64));
        assert_eq!(
            unwrapper.unwrap(crate::Point::from_raw(-2)).unwrap().raw(),
            -2
        );
        let _ = UnixMicros::<i64>::from_raw(0);
    }

    #[test]
    fn u16_ms_without_anchor() {
        let mut unwrapper = TimeUnwrapper::<DeviceMillis<u16>>::default();
        assert_eq!(unwrapper.last(), None);
        // every 20 s for 10 minutes: u16 ms wraps every 65.5 s
        for i in 0..30u64 {
            let t = 1_000 + i * 20_000;
            let full = unwrapper
                .unwrap(DeviceMillis::from_raw((t % 65536) as u16))
                .unwrap();
            assert_eq!(full.raw(), t);
        }
    }

    #[test]
    fn full_width_passes_through_and_restart_forgets() {
        let mut unwrapper = TimeUnwrapper::<DeviceMicros<u64>>::default();
        let t = DeviceMicros::from_raw(u64::MAX);
        assert_eq!(unwrapper.unwrap(t), Ok(t));
        let mut unwrapper = TimeUnwrapper::<DeviceMicros<u32>>::default();
        unwrapper.unwrap(us32(u32::MAX as u64)).unwrap();
        assert_eq!(unwrapper.unwrap(us32(5)).unwrap().raw(), (1 << 32) + 5);
        // the device rebooted: its clock counts from 0 again
        unwrapper.restart();
        assert_eq!(unwrapper.unwrap(us32(5)).unwrap().raw(), 5);
    }

    #[test]
    fn past_64_bits_overflows() {
        let mut unwrapper = TimeUnwrapper::new(DeviceMicros::<u64>::from_raw(u64::MAX - 2));
        assert_eq!(unwrapper.unwrap(us32(10)), Err(TimeOverflow));
    }
}

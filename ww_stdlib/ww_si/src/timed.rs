//! [Timed]: a value with the time it was taken, and [TimePoint]: what that time can be.

use exact_units::origins::DeviceMicros;
use exact_units::units::Second;
use exact_units::{Fixed, FixedScale, Linear, Origin, Point};
use shrink_wrap::prelude::*;

/// A value with the time it was taken: a sample of a property, an item of a stream (`Timed<Ampere>`,
/// `Timed<IvSample>`).
///
/// `t` is a moment on a time scale, a [Point] in seconds: by default a [DeviceMicros]`<u32>`, µs since the device
/// booted (`DeviceBoot`) in a `u32` that wraps every 71.6 minutes. The host gets the full time back as a
/// `DeviceMicros<u64>` with [TimeUnwrapper](crate::TimeUnwrapper). A device picks another `Tm` for a stream that is
/// quiet for longer than half a wrap or needs another resolution (`DeviceMillis<u32>` wraps every 49.7 days,
/// `DeviceMicros<u64>` never), or a time scale it knows (`UnixMicros<u64>` for a device that keeps UTC): any
/// [TimePoint].
///
/// On the wire it is a `final_structure` struct of `t`, the bytes of its storage alone, and then `value`: no fields
/// can be added later. In a `Series` (`series`, SW-60) `t` is a column like any other: a regular clock costs nothing
/// per item.
///
/// ```
/// use ww_si::{Ampere, Timed, origins::DeviceMicros};
///
/// let sample = Timed::new(DeviceMicros::from_raw(1_000_040u32), Ampere::milli(250u16));
/// assert_eq!(sample.t.raw(), 1_000_040);
/// ```
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq),
    final_structure,
    series(cells)
)]
pub struct Timed<V, Tm = DeviceMicros<u32>> {
    /// When `value` was taken.
    pub t: Tm,
    pub value: V,
}

impl<V, Tm> Timed<V, Tm> {
    pub const fn new(t: Tm, value: V) -> Self {
        Timed { t, value }
    }
}

/// A moment on a time scale that a [Timed] can carry: a [Point] in [Second]s of a [Linear] origin, in integer steps
/// of `P`, without bounds. [DeviceMicros]`<u32>` (the default of [Timed]), `DeviceMillis<u16>`, `UnixMicros<u64>`,
/// `Point<DeviceBoot, Fixed<Second, Pow2<-15>, u32>>` (a 32768 Hz RTC).
///
/// A storage narrower than 64 bits holds the point's low bits: the device's clock wraps and the host puts the high
/// bits back ([TimeUnwrapper](crate::TimeUnwrapper)) into [Full](Self::Full), the same point in 64 bits. On the device
/// only the wrapping operations here are used ([wrapping_add](Self::wrapping_add), [steps](Self::steps)); `+` of a
/// `u32` point panics in debug builds where the clock wraps.
pub trait TimePoint: Copy {
    /// The origin: `DeviceBoot`, `UnixEpoch`, ...
    type Origin: Origin<Unit = Second, Modulus = Linear>;
    /// The step of the number: `Micro`, `Milli`, `Pow2<-15>`.
    type Step: FixedScale;
    /// The storage on the wire: `u32`.
    type Storage: exact_units::Storage;
    /// The same point in 64 bits (`u64`, `i64` for a signed storage): what the host unwraps to.
    type Full: TimePoint<Origin = Self::Origin, Step = Self::Step>;

    /// Bits of [Storage](Self::Storage): the point wraps every `2^BITS` steps, 64 never.
    const BITS: u32;

    /// The number as an integer: the raw value, not unwrapped.
    fn raw_int(self) -> i128;

    /// The point whose number is `raw` modulo `2^BITS`.
    fn from_int_wrapping(raw: i128) -> Self;

    /// The point `full` in this storage: its low bits.
    fn low(full: Self::Full) -> Self {
        Self::from_int_wrapping(full.raw_int())
    }

    /// `self + dt`, wrapping like the clock.
    fn wrapping_add(self, dt: Fixed<Second, Self::Step, Self::Storage>) -> Self;

    /// `self`, `self + dt`, `self + 2·dt`, ...: the times of a regularly sampled block (an ADC DMA half-transfer),
    /// wrapping like the clock.
    fn steps(
        self,
        dt: Fixed<Second, Self::Step, Self::Storage>,
    ) -> impl Iterator<Item = Self> + Clone {
        core::iter::successors(Some(self), move |t| Some(t.wrapping_add(dt)))
    }
}

/// The step between two [TimePoint]s `Tm` in the same steps and storage: `Fixed<Second, Micro, u32>` for a
/// `DeviceMicros<u32>`. What [TimePoint::steps] and a batched stream's `push_many` take as `dt`.
pub type TimeStep<Tm> = Fixed<Second, <Tm as TimePoint>::Step, <Tm as TimePoint>::Storage>;

macro_rules! time_point {
    ($full:ty: $($t:ty)*) => {$(
        impl<O, P> TimePoint for Point<O, Fixed<Second, P, $t>>
        where
            O: Origin<Unit = Second, Modulus = Linear>,
            P: FixedScale,
        {
            type Origin = O;
            type Step = P;
            type Storage = $t;
            type Full = Point<O, Fixed<Second, P, $full>>;

            const BITS: u32 = <$t>::BITS;

            #[inline(always)]
            fn raw_int(self) -> i128 {
                i128::from(self.raw())
            }

            #[inline(always)]
            #[expect(clippy::cast_possible_truncation, reason = "the low bits are the point")]
            fn from_int_wrapping(raw: i128) -> Self {
                Point::from_raw(raw as $t)
            }

            #[inline(always)]
            fn wrapping_add(self, dt: Fixed<Second, P, $t>) -> Self {
                // named by its full self type: exact_units also has an inherent wrapping_add on
                // Point<_, Scaled<_, $t>>, ambiguous with this one when $t is inferred alone
                Point::<O, Fixed<Second, P, $t>>::wrapping_add(self, dt)
            }
        }
    )*};
}

time_point!(u64: u8 u16 u32 u64);
time_point!(i64: i8 i16 i32 i64);

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use exact_units::origins::{DeviceBoot, DeviceMillis, UnixMicros};
    use exact_units::prefix::{Micro, Pow2};

    #[test]
    fn steps_wrap_like_the_clock() {
        let t0 = DeviceMicros::<u32>::from_raw(u32::MAX - 15);
        let dt = Fixed::<Second, Micro, u32>::new(10);
        let times: Vec<u32> = t0.steps(dt).take(3).map(|t| t.raw()).collect();
        assert_eq!(times, [u32::MAX - 15, u32::MAX - 5, 4]);
    }

    #[test]
    fn low_bits_of_the_full_point() {
        let full = DeviceMicros::<u64>::from_raw(5_000_000_123);
        assert_eq!(DeviceMicros::<u32>::low(full).raw(), 705_032_827);
        assert_eq!(
            DeviceMillis::<u16>::low(DeviceMillis::from_raw(70_000)).raw(),
            4464
        );
        let utc = UnixMicros::<i64>::from_raw(-5);
        assert_eq!(UnixMicros::<i64>::low(utc), utc);
        type Rtc = Point<DeviceBoot, Fixed<Second, Pow2<-15>, u32>>;
        assert_eq!(<Rtc as TimePoint>::BITS, 32);
    }
}

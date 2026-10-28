//! [Fixed]: a number with its unit, prefix and storage fixed at compile time, on the wire exactly the number.

use crate::tree::write_unit;
use crate::unit::{Unit, UnitExpr};
use crate::{Magnitude, Prefix};
use core::cmp::Ordering;
use core::fmt;
use core::hash::{Hash, Hasher};
use core::marker::PhantomData;
#[cfg(feature = "shrink_wrap")]
use shrink_wrap::prelude::*;

pub use bounds::{
    Bounds, OutOfBounds, Range, RangeDec, RangeU64, Saturating, Unbounded, Unlimited, Wrapping,
};
pub use prefix::{DecimalPrefix, FixedScale};
pub use rounding::{Overflow, Requantizer, Rounding, StorageKind, requantize};
pub use storage::{Repr, Storage, UnsignedStorage};

mod bounds;
pub mod prefix;
mod rounding;
mod storage;

/// A value in unit `U` and prefix `P`, stored as `T`, within the bounds `B`: on the wire exactly `T`, the unit,
/// prefix and bounds are only in the type. `shared_us: u64` becomes `shared: Fixed<Second, Micro, u64>` (or
/// [Micros]`<u64>`) with the same bytes, and introspection shows `µs`.
///
/// Where a `ww_si::Quantity` lets the device pick prefix and number type per value (and sends both), a `Fixed` has them fixed
/// by the API: no prefix on the wire, no `NumericValue` on the device, so a `u64` here doesn't turn on `ww_numeric`'s
/// `u64` for the whole firmware.
///
/// ```
/// use exact_units::{Fixed, Micros, prefix::{Micro, Milli}, units::Second};
///
/// let t: Micros<u32> = Fixed::new(1500);
/// assert_eq!(format!("{t}"), "1500 µs");
/// assert_eq!(format!("{t:#}"), "0.0015 s");
/// // exact conversions only: 1500 µs is not a whole number of ms
/// assert_eq!(t.to::<Milli, u16>(), None);
/// assert_eq!((t * 2).to::<Milli, u16>(), Some(Fixed::new(3)));
/// assert_eq!(core::time::Duration::from(t), core::time::Duration::from_micros(1500));
/// ```
///
/// `B` is [Unbounded] unless given, and stays out of the way then: `Fixed::new` and the operators exist for the
/// three-parameter form. A bounded value is made with [try_new](Self::try_new) or [bounded](Self::bounded) and checked
/// on every read from the wire:
///
/// ```
/// use exact_units::{Fixed, Range, prefix::Milli, units::Second};
///
/// type Timeout = Fixed<Second, Milli, u16, Range<1, 5000>>;
/// assert!(Timeout::try_new(10).is_ok());
/// assert!(Timeout::try_new(0).is_err());
/// const LONG: Timeout = Timeout::bounded(5000);
/// assert_eq!(LONG.get(), 5000);
/// ```
///
/// A bound that doesn't fit the storage doesn't build:
///
/// ```compile_fail
/// use exact_units::{Fixed, Range, prefix::One, units::Ratio};
///
/// let _ = Fixed::<Ratio, One, u8, Range<-5, 300>>::try_new(5);
/// ```
#[cfg_attr(
    feature = "shrink_wrap",
    derive_shrink_wrap(
        borrowed,
        owned(feature = "std"),
        transparent,
        series(cells),
        validate = Self::validate
    )
)]
#[repr(transparent)]
pub struct Fixed<U, P, T: Storage, B: Bounds<T> = Unbounded> {
    value: T,
    _unit: PhantomData<U>,
    _prefix: PhantomData<P>,
    _bounds: PhantomData<B>,
}

/// Time in µs, `Fixed<Second, Micro, T>`.
#[cfg(feature = "time")]
pub type Micros<T> = Fixed<crate::units::Second, prefix::Micro, T>;

/// Time in ms, `Fixed<Second, Milli, T>`.
#[cfg(feature = "time")]
pub type Millis<T> = Fixed<crate::units::Second, prefix::Milli, T>;

/// Time in ns, `Fixed<Second, Nano, T>`.
#[cfg(feature = "time")]
pub type Nanos<T> = Fixed<crate::units::Second, prefix::Nano, T>;

/// A size or a count of bytes, `Fixed<Byte, One, T>`.
#[cfg(feature = "information")]
pub type Bytes<T> = Fixed<crate::units::Byte, prefix::One, T>;

/// A 16-bit duty or fraction where 65535 is 1 (100 %): `Fixed<Ratio, Rational<1, 65535>, u16>`, the PWM duty of
/// many timers.
#[cfg(feature = "dimensionless")]
pub type Duty16 = Fixed<crate::units::Ratio, prefix::Rational<1, 65535>, u16>;

/// A Q15 fraction (CMSIS-DSP `q15_t`): `i16` in steps of 2⁻¹⁵, -1 to 1 - 2⁻¹⁵. `Q15<Volt>` is a voltage in Q15.
/// Arithmetic saturates ([Saturating]), as Q formats do.
#[cfg(feature = "dimensionless")]
pub type Q15<U = crate::units::Ratio> = Fixed<U, prefix::Pow2<-15>, i16, Saturating>;

/// A Q31 fraction (CMSIS-DSP `q31_t`): `i32` in steps of 2⁻³¹, saturating.
#[cfg(feature = "dimensionless")]
pub type Q31<U = crate::units::Ratio> = Fixed<U, prefix::Pow2<-31>, i32, Saturating>;

/// UQ16.16: `u32` with 16 integer and 16 fraction bits, steps of 2⁻¹⁶, saturating.
#[cfg(feature = "dimensionless")]
pub type UQ16_16<U = crate::units::Ratio> = Fixed<U, prefix::Pow2<-16>, u32, Saturating>;

/// A 16-bit binary angle (BAM): 65536 steps make a turn, `Fixed<Turn, Pow2<-16>, u16>`, ST's `s16degree` read as
/// unsigned. Arithmetic wraps at a turn ([Wrapping]): 270° + 180° is 90°.
#[cfg(feature = "angle")]
pub type Bam16 = Fixed<crate::units::Turn, prefix::Pow2<-16>, u16, Wrapping>;

/// A 32-bit binary angle: 2³² steps make a turn, wrapping.
#[cfg(feature = "angle")]
pub type Bam32 = Fixed<crate::units::Turn, prefix::Pow2<-32>, u32, Wrapping>;

/// A fraction in parts per billion (a clock's rate error), `Fixed<Ratio, Nano, T>`.
#[cfg(feature = "dimensionless")]
pub type Ppb<T> = Fixed<crate::units::Ratio, prefix::Nano, T>;

impl<U, P, T: Storage> Fixed<U, P, T> {
    /// `value` in units of `P`.
    #[inline(always)]
    pub const fn new(value: T) -> Self {
        Fixed {
            value,
            _unit: PhantomData,
            _prefix: PhantomData,
            _bounds: PhantomData,
        }
    }
}

impl<U, P, T: Storage> From<T> for Fixed<U, P, T> {
    #[inline(always)]
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<U, P, T: Storage + Default, B: Unlimited<T>> Default for Fixed<U, P, T, B> {
    fn default() -> Self {
        Self::new_unchecked(T::default())
    }
}

impl<U, P, T: Storage, B: Unlimited<T>> Fixed<U, P, T, B> {
    /// `value` in units of `P`, for every bounds marker without limits: `new` of [Wrapping] and [Saturating]
    /// (`Bam16::of(16384)`), which can't share the name with [Unbounded]'s `new` without making `Fixed::new` ambiguous.
    #[inline(always)]
    pub const fn of(value: T) -> Self {
        Self::new_unchecked(value)
    }

    /// The same value with the overflow policy of `B2`: [Unbounded] (checked), [Wrapping] or [Saturating]. Free.
    #[inline(always)]
    pub const fn with_overflow<B2: Unlimited<T>>(self) -> Fixed<U, P, T, B2> {
        Fixed::new_unchecked(self.value)
    }
}

impl<U, P, T: Storage, B: Bounds<T>> Fixed<U, P, T, B> {
    /// The number in units of `P`.
    #[inline(always)]
    pub const fn get(self) -> T {
        self.value
    }

    /// Without the bounds check: for bounds checked some other way.
    #[inline(always)]
    pub(crate) const fn new_unchecked(value: T) -> Self {
        Fixed {
            value,
            _unit: PhantomData,
            _prefix: PhantomData,
            _bounds: PhantomData,
        }
    }
}

impl<U, P, T: Storage, B: Bounds<T>> Fixed<U, P, T, B> {
    /// `value` in units of `P`, if it is within `B`.
    #[inline(always)]
    pub fn try_new(value: T) -> Result<Self, OutOfBounds> {
        if B::contains(value) {
            Ok(Self::new_unchecked(value))
        } else {
            Err(OutOfBounds)
        }
    }

    /// The same value without bounds.
    #[inline(always)]
    pub fn unbounded(self) -> Fixed<U, P, T> {
        Fixed::new(self.value)
    }

    /// Run on every value read from the wire: a value outside `B` is [shrink_wrap::Error::SubtypeOutOfRange].
    #[cfg(feature = "shrink_wrap")]
    #[inline(always)]
    fn validate(&self) -> Result<(), ShrinkWrapError> {
        if B::contains(self.value) {
            Ok(())
        } else {
            Err(ShrinkWrapError::SubtypeOutOfRange)
        }
    }
}

impl<U: Unit, P: FixedScale, T: Storage, B: Bounds<T>> Fixed<U, P, T, B> {
    /// The same value in scale `P2` and storage `T2`, if that is exact: `None` when it is not a whole number of
    /// `P2` for an integer `T2`, or doesn't fit `T2`. Floats are scaled.
    ///
    /// The factor `P / P2` is worked out at compile time, so a conversion costs what the same multiply and divide
    /// by constants written by hand cost (nothing between equal scales: `Milli` to `Pow10<-3>`). Between integer
    /// storages it must be a fraction of integers whose multiply fits 64 bits (both storages of up to 32 bits) or
    /// 128 bits, else the conversion doesn't compile; it is never rounded:
    ///
    /// ```compile_fail
    /// use exact_units::{Fixed, prefix::{One, Pow10}, units::Second};
    /// // 10⁴⁰ · u32::MAX doesn't fit 64 bits
    /// let _ = Fixed::<Second, Pow10<20>, u32>::new(1).to::<Pow10<-20>, u32>();
    /// ```
    ///
    /// ```compile_fail
    /// use exact_units::{Fixed, prefix::One, units::Radian};
    ///
    /// exact_units::scale! {
    ///     /// Steps of π rad.
    ///     struct PiSteps = "", Exact(1 / 1, pi 1);
    /// }
    /// // π is not a fraction of integers: in an integer storage that is a compile error, in a float it scales
    /// let _ = Fixed::<Radian, PiSteps, i32>::new(1).to::<One, i32>();
    /// ```
    #[inline(always)]
    pub fn to<P2: FixedScale, T2: Storage>(self) -> Option<Fixed<U, P2, T2>> {
        let c = const { Conversion::of::<P, P2, T, T2>(false) };
        c.apply::<T, T2>(self.value).map(Fixed::new)
    }

    /// The same value in scale `P2` and storage `T2`, rounded by `r` where it isn't a whole number of `P2`: `None`
    /// only when it doesn't fit `T2` ([Overflow::Checked]; [to_saturating](Self::to_saturating) and
    /// [to_wrapping](Self::to_wrapping) are the other policies).
    ///
    /// ```
    /// use exact_units::{Fixed, Rounding, prefix::{Micro, Milli}, units::Second};
    ///
    /// let t: Fixed<Second, Micro, i32> = Fixed::new(-2500);
    /// assert_eq!(t.to::<Milli, i16>(), None); // not exact
    /// let ms = |r| t.to_rounded::<Milli, i16>(r).map(Fixed::get);
    /// assert_eq!(ms(Rounding::Truncate), Some(-2));
    /// assert_eq!(ms(Rounding::Floor), Some(-3));
    /// assert_eq!(ms(Rounding::Ceil), Some(-2));
    /// assert_eq!(ms(Rounding::NearestEven), Some(-2));
    /// assert_eq!(ms(Rounding::NearestAway), Some(-3));
    /// ```
    ///
    /// The factor is worked out at compile time as for [to](Self::to), and the rounding is exact: the true quotient,
    /// rounded once. Between integer storages a factor that isn't a fraction of integers (a power of π between
    /// radians and degrees, ln 2, ln 10, a half exponent) is allowed here, unlike in `to`: it is multiplied as its
    /// 64-bit fixed point ([Magnitude::fixed_point]), within 2⁻⁶⁴ of the factor, so the result is the correctly
    /// rounded one except where the true value lies within `|x| · 2⁻⁶⁴` of a rounding step, and then one step off.
    /// With a float on either side the float result is rounded.
    #[inline(always)]
    pub fn to_rounded<P2: FixedScale, T2: Storage>(self, r: Rounding) -> Option<Fixed<U, P2, T2>> {
        self.requantize(r, Overflow::Checked)
    }

    /// As [to_rounded](Self::to_rounded), at the nearest end of `T2` where it doesn't fit (a `NaN` float is 0).
    #[inline(always)]
    pub fn to_saturating<P2: FixedScale, T2: Storage>(self, r: Rounding) -> Fixed<U, P2, T2> {
        let c = const { Conversion::of::<P, P2, T, T2>(true) };
        Fixed::new(
            c.apply_rounded(self.value, r, Overflow::Saturate)
                .unwrap_or(T2::ZERO),
        )
    }

    /// As [to_rounded](Self::to_rounded), modulo 2^bits of `T2` where it doesn't fit (two's complement for a signed
    /// `T2`; a `NaN` or infinite float is 0).
    #[inline(always)]
    pub fn to_wrapping<P2: FixedScale, T2: Storage>(self, r: Rounding) -> Fixed<U, P2, T2> {
        let c = const { Conversion::of::<P, P2, T, T2>(true) };
        Fixed::new(
            c.apply_rounded(self.value, r, Overflow::Wrap)
                .unwrap_or(T2::ZERO),
        )
    }

    /// The same value in scale `P2` and storage `T2`, rounded by `r` and settled by `o`: the three methods above in
    /// one, for a policy chosen at run time. `None` only for [Overflow::Checked].
    #[inline(always)]
    pub fn requantize<P2: FixedScale, T2: Storage>(
        self,
        r: Rounding,
        o: Overflow,
    ) -> Option<Fixed<U, P2, T2>> {
        let c = const { Conversion::of::<P, P2, T, T2>(true) };
        c.apply_rounded(self.value, r, o).map(Fixed::new)
    }

    /// The same value in scale `P2` and storage `T2`, rounded by `r`, infallible: the bounds `B` (or the whole range
    /// of `T` without them) prove at compile time that every rounded value fits `T2`, else this doesn't compile.
    /// Integer storages only.
    ///
    /// ```
    /// use exact_units::{Fixed, Range, Rounding, prefix::{Milli, One}, units::Volt};
    ///
    /// // 0 to 3.3 V in mV rounds into a u8 of 0.1 V steps, whatever the rounding
    /// type Rail = Fixed<Volt, Milli, i16, Range<0, 3300>>;
    /// let v = Rail::bounded(1250);
    /// let dv: Fixed<Volt, exact_units::prefix::Deci, u8> = v.into_rounded(Rounding::NearestEven);
    /// assert_eq!(dv.get(), 12);
    /// ```
    ///
    /// ```compile_fail
    /// use exact_units::{Fixed, Rounding, prefix::{Milli, Deci}, units::Volt};
    /// // an i16 of mV reaches 3276.7 V: more than a u8 of 0.1 V steps holds
    /// let v: Fixed<Volt, Milli, i16> = Fixed::new(1250);
    /// let _: Fixed<Volt, Deci, u8> = v.into_rounded(Rounding::NearestEven);
    /// ```
    #[inline(always)]
    pub fn into_rounded<P2: FixedScale, T2: Storage>(self, r: Rounding) -> Fixed<U, P2, T2> {
        let c = const { Conversion::proven::<P, P2, T, T2, B>(false) };
        // proven in range: wrapping is the plain cast
        Fixed::new(
            c.apply_rounded(self.value, r, Overflow::Wrap)
                .unwrap_or(T2::ZERO),
        )
    }

    /// The same value in another unit `U2` of the same dimension, scale `P2` and storage `T2`, if that is exact: a
    /// pound-force second in newton seconds. `None` as for [to](Self::to); never a value reinterpreted in the other
    /// unit. Both units need an exact factor to SI ([Scale::Exact](crate::Scale), a power of ten or coherent), and
    /// the factor `U · P / (U2 · P2)` is worked out at compile time, so a conversion costs what [to](Self::to) costs.
    ///
    /// ```
    /// # #[cfg(feature = "customary")] {
    /// use exact_units::{Fixed, prefix::{One, Pico}, units::{Newton, PoundForce, Second, Times}};
    ///
    /// // 1 lbf·s is 4.4482216152605 N·s exactly
    /// let impulse = Fixed::<Times<PoundForce, Second>, One, i64>::new(2);
    /// assert_eq!(
    ///     impulse.convert::<Times<Newton, Second>, Pico, i64>(),
    ///     Some(Fixed::new(8_896_443_230_521))
    /// );
    /// // 8.896443230521 N·s is not a whole number of N·s
    /// assert_eq!(impulse.convert::<Times<Newton, Second>, One, i64>(), None);
    /// // in a float it scales
    /// let f = impulse.convert::<Times<Newton, Second>, One, f64>().unwrap();
    /// assert!((f.get() - 8.896443230521).abs() < 1e-12);
    /// # }
    /// ```
    ///
    /// Units of different dimensions don't convert, a pound of force is not a pound of mass:
    ///
    /// ```compile_fail
    /// use exact_units::{Fixed, prefix::One, units::{PoundAvdp, PoundForce}};
    /// let _ = Fixed::<PoundForce, One, i32>::new(1).convert::<PoundAvdp, One, i32>();
    /// ```
    #[inline(always)]
    pub fn convert<U2: Unit, P2: FixedScale, T2: Storage>(self) -> Option<Fixed<U2, P2, T2>> {
        let c = const { Conversion::between::<U, P, U2, P2, T, T2>() };
        c.apply::<T, T2>(self.value).map(Fixed::new)
    }

    /// The value `x · scale` (in the unit `U` itself) in this prefix and storage, if it is exact (as [to](Self::to))
    /// and within `B`: how a number with a run-time scale becomes a `Fixed`, `Repr::Int(3)` with `Pow10(-3)` is
    /// 3000 of `Micro`.
    pub fn from_scaled(x: Repr, scale: Magnitude) -> Option<Self> {
        Self::try_new(rescale(x, scale.div(P::SCALE))?).ok()
    }

    /// The same value with other bounds, if it is within them.
    pub fn to_bounds<B2: Bounds<T>>(self) -> Result<Fixed<U, P, T, B2>, OutOfBounds> {
        Fixed::try_new(self.value)
    }

    /// `self + rhs`, `None` on overflow or outside `B`.
    #[inline(always)]
    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        Self::try_new(self.value.checked_add(rhs.value)?).ok()
    }

    /// `self - rhs`, `None` on overflow or outside `B`.
    #[inline(always)]
    pub fn checked_sub(self, rhs: Self) -> Option<Self> {
        Self::try_new(self.value.checked_sub(rhs.value)?).ok()
    }

    /// `self * n`, `None` on overflow or outside `B`.
    #[inline(always)]
    pub fn checked_mul(self, n: T) -> Option<Self> {
        Self::try_new(self.value.checked_mul(n)?).ok()
    }

    /// `self / n`, `None` for `n == 0` (integers) or outside `B`.
    #[inline(always)]
    pub fn checked_div(self, n: T) -> Option<Self> {
        Self::try_new(self.value.checked_div(n)?).ok()
    }

    /// `self + rhs`, at the edge of `T` or `B` where it would leave them.
    #[inline]
    pub fn saturating_add(self, rhs: Self) -> Self {
        Self::clamped(self.value.saturating_add(rhs.value))
    }

    /// `self - rhs`, at the edge of `T` or `B` where it would leave them.
    #[inline]
    pub fn saturating_sub(self, rhs: Self) -> Self {
        Self::clamped(self.value.saturating_sub(rhs.value))
    }

    /// `self * n`, at the edge of `T` or `B` where it would leave them.
    #[inline]
    pub fn saturating_mul(self, n: T) -> Self {
        Self::clamped(self.value.saturating_mul(n))
    }

    #[inline(always)]
    fn clamped(mut x: T) -> Self {
        if let Some(min) = B::MIN
            && x < min
        {
            x = min;
        }
        if let Some(max) = B::MAX
            && x > max
        {
            x = max;
        }
        Self::new_unchecked(x)
    }
}

impl<U, P, T: Storage, B: Unlimited<T>> Fixed<U, P, T, B> {
    /// `self + rhs`, wrapping around at the edge of `T` like a counter: `u32::MAX` µs + 2 µs is 1 µs. Only without
    /// bounds; what `+` is for [Wrapping].
    #[inline(always)]
    pub fn wrapping_add(self, rhs: Self) -> Self {
        Self::new_unchecked(self.value.wrapping_add(rhs.value))
    }

    /// `self - rhs`, wrapping around at the edge of `T`. Only without bounds.
    #[inline(always)]
    pub fn wrapping_sub(self, rhs: Self) -> Self {
        Self::new_unchecked(self.value.wrapping_sub(rhs.value))
    }

    /// `self * n`, wrapping around at the edge of `T`. Only without bounds.
    #[inline(always)]
    pub fn wrapping_mul(self, n: T) -> Self {
        Self::new_unchecked(self.value.wrapping_mul(n))
    }
}

impl<U, P, T: Storage, B: Bounds<T>> Clone for Fixed<U, P, T, B> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<U, P, T: Storage, B: Bounds<T>> Copy for Fixed<U, P, T, B> {}

/// A bounded value into more steps of a wider storage, exactly and infallibly: the factor `P / P2` must be whole and
/// the range times it must fit `T2`, both checked at compile time where the conversion is used (else it doesn't
/// compile, use [into_rounded](Fixed::into_rounded) or [to](Fixed::to)). An unbounded source has no `From`, it would
/// overlap `From<T> for T`; [into_rounded](Fixed::into_rounded) is the same conversion there.
///
/// ```
/// use exact_units::{Fixed, Range, prefix::{Micro, Milli}, units::Volt};
///
/// let v = Fixed::<Volt, Milli, i16, Range<0, 3300>>::bounded(3300);
/// let uv: Fixed<Volt, Micro, i32> = v.into();
/// assert_eq!(uv.get(), 3_300_000);
/// ```
///
/// ```compile_fail
/// use exact_units::{Fixed, Range, prefix::{Micro, Milli}, units::Volt};
/// // 3300 mV · 1000 doesn't fit an i16
/// let v = Fixed::<Volt, Milli, i16, Range<0, 3300>>::bounded(3300);
/// let _: Fixed<Volt, Micro, i16> = v.into();
/// ```
impl<U, P, T, P2, T2, const MIN: i64, const MAX: i64> From<Fixed<U, P, T, Range<MIN, MAX>>>
    for Fixed<U, P2, T2>
where
    P: FixedScale,
    P2: FixedScale,
    T: Storage,
    T2: Storage,
    Range<MIN, MAX>: Bounds<T>,
{
    #[inline(always)]
    fn from(x: Fixed<U, P, T, Range<MIN, MAX>>) -> Self {
        let c = const { Conversion::proven::<P, P2, T, T2, Range<MIN, MAX>>(true) };
        Fixed::new(
            c.apply_rounded(x.value, Rounding::Truncate, Overflow::Wrap)
                .unwrap_or(T2::ZERO),
        )
    }
}

impl<U, P, T: Storage, B: Bounds<T>> PartialEq for Fixed<U, P, T, B> {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<U, P, T: Storage + Eq, B: Bounds<T>> Eq for Fixed<U, P, T, B> {}

impl<U, P, T: Storage, B: Bounds<T>> PartialOrd for Fixed<U, P, T, B> {
    #[inline(always)]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.value.partial_cmp(&other.value)
    }
}

impl<U, P, T: Storage + Ord, B: Bounds<T>> Ord for Fixed<U, P, T, B> {
    #[inline(always)]
    fn cmp(&self, other: &Self) -> Ordering {
        self.value.cmp(&other.value)
    }
}

impl<U, P, T: Storage + Hash, B: Bounds<T>> Hash for Fixed<U, P, T, B> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.value.hash(state)
    }
}

/// `+` and `-` of the same type, `*` and `/` by a number, by the overflow policy in `B`: for [Unbounded] like the
/// same operator on `T` (panics on overflow in debug builds), for [Wrapping] modulo 2^bits, for [Saturating] at the
/// ends of `T`. A bounded value has [checked_add](Fixed::checked_add) and friends.
mod arith {
    use super::*;
    use core::iter::Sum;
    use core::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Sub, SubAssign};

    macro_rules! policy {
        ($($b:ident $add:ident $sub:ident $mul:ident $div:ident;)*) => {$(
            impl<U, P, T: Storage> Add for Fixed<U, P, T, $b> {
                type Output = Self;
                #[inline(always)]
                fn add(self, rhs: Self) -> Self {
                    Self::of(self.value.$add(rhs.value))
                }
            }

            impl<U, P, T: Storage> Sub for Fixed<U, P, T, $b> {
                type Output = Self;
                #[inline(always)]
                fn sub(self, rhs: Self) -> Self {
                    Self::of(self.value.$sub(rhs.value))
                }
            }

            impl<U, P, T: Storage> Mul<T> for Fixed<U, P, T, $b> {
                type Output = Self;
                #[inline(always)]
                fn mul(self, n: T) -> Self {
                    Self::of(self.value.$mul(n))
                }
            }

            /// Division by 0 panics as for `T`; only `MIN / -1` of a signed `T` overflows.
            impl<U, P, T: Storage> Div<T> for Fixed<U, P, T, $b> {
                type Output = Self;
                #[inline(always)]
                fn div(self, n: T) -> Self {
                    Self::of(self.value.$div(n))
                }
            }

            impl<U, P, T: Storage> AddAssign for Fixed<U, P, T, $b> {
                #[inline(always)]
                fn add_assign(&mut self, rhs: Self) {
                    *self = *self + rhs;
                }
            }

            impl<U, P, T: Storage> SubAssign for Fixed<U, P, T, $b> {
                #[inline(always)]
                fn sub_assign(&mut self, rhs: Self) {
                    *self = *self - rhs;
                }
            }

            impl<U, P, T: Storage> MulAssign<T> for Fixed<U, P, T, $b> {
                #[inline(always)]
                fn mul_assign(&mut self, n: T) {
                    *self = *self * n;
                }
            }

            impl<U, P, T: Storage> DivAssign<T> for Fixed<U, P, T, $b> {
                #[inline(always)]
                fn div_assign(&mut self, n: T) {
                    *self = *self / n;
                }
            }

            impl<U, P, T: Storage> Sum for Fixed<U, P, T, $b> {
                fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
                    iter.fold(Self::of(T::ZERO), |a, b| a + b)
                }
            }
        )*};
    }

    policy! {
        Wrapping wrapping_add wrapping_sub wrapping_mul wrapping_div;
        Saturating saturating_add saturating_sub saturating_mul saturating_div;
    }

    impl<U, P, T: Storage> Add for Fixed<U, P, T> {
        type Output = Self;
        #[inline(always)]
        fn add(self, rhs: Self) -> Self {
            Self::new(self.value.add(rhs.value))
        }
    }

    impl<U, P, T: Storage> Sub for Fixed<U, P, T> {
        type Output = Self;
        #[inline(always)]
        fn sub(self, rhs: Self) -> Self {
            Self::new(self.value.sub(rhs.value))
        }
    }

    impl<U, P, T: Storage> Mul<T> for Fixed<U, P, T> {
        type Output = Self;
        #[inline(always)]
        fn mul(self, n: T) -> Self {
            Self::new(self.value.mul(n))
        }
    }

    impl<U, P, T: Storage> Div<T> for Fixed<U, P, T> {
        type Output = Self;
        #[inline(always)]
        fn div(self, n: T) -> Self {
            Self::new(self.value.div(n))
        }
    }

    impl<U, P, T: Storage> AddAssign for Fixed<U, P, T> {
        #[inline(always)]
        fn add_assign(&mut self, rhs: Self) {
            *self = *self + rhs;
        }
    }

    impl<U, P, T: Storage> SubAssign for Fixed<U, P, T> {
        #[inline(always)]
        fn sub_assign(&mut self, rhs: Self) {
            *self = *self - rhs;
        }
    }

    impl<U, P, T: Storage> MulAssign<T> for Fixed<U, P, T> {
        #[inline(always)]
        fn mul_assign(&mut self, n: T) {
            *self = *self * n;
        }
    }

    impl<U, P, T: Storage> DivAssign<T> for Fixed<U, P, T> {
        #[inline(always)]
        fn div_assign(&mut self, n: T) {
            *self = *self / n;
        }
    }

    impl<U, P, T: Storage> Sum for Fixed<U, P, T> {
        fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
            iter.fold(Self::new(T::ZERO), |a, b| a + b)
        }
    }
}

/// The factor between two scales and storages, worked out at compile time by [Fixed::to].
#[derive(Copy, Clone)]
struct Conversion {
    num: u128,
    den: u128,
    /// The widened multiply in 128 bits (a storage of 64 bits), else in 64.
    wide: bool,
    /// A float storage on either side: scaled by `num_f / den_f`, or by `factor` when that isn't a fraction of
    /// integers both floats hold exactly.
    float: bool,
    exact_f: bool,
    num_f: f64,
    den_f: f64,
    factor: f64,
    /// Between integer storages, a factor that isn't a fraction of integers as `m · 2^shift`
    /// ([Magnitude::fixed_point]): only for a rounding conversion.
    fixed_point: Option<(u64, i32)>,
}

impl Conversion {
    /// The conversion `P` → `P2`, `T` → `T2`; `rounding` allows a factor that isn't a fraction of integers between
    /// integer storages, by its fixed point.
    const fn of<P: FixedScale, P2: FixedScale, T: Storage, T2: Storage>(rounding: bool) -> Self {
        Self::by::<T, T2>(P::SCALE.div(P2::SCALE), rounding)
    }

    /// From unit `U` in scale `P` to `U2` in `P2`: both units' exact factors to SI and the same dimension, else a
    /// compile error.
    const fn between<U: Unit, P: FixedScale, U2: Unit, P2: FixedScale, T: Storage, T2: Storage>()
    -> Self {
        assert!(
            U::DIM.same(U2::DIM),
            "a Fixed converts only between units of the same dimension (a pound-force is not a pound)"
        );
        assert!(
            !U::EXPR.has_offset() && !U2::EXPR.has_offset(),
            "a unit with an offset converts as a Point, not a Fixed"
        );
        let (Some(from), Some(to)) = (U::EXPR.magnitude(), U2::EXPR.magnitude()) else {
            panic!("a Fixed converts only between units with an exact factor to SI")
        };
        Self::by::<T, T2>(from.mul(P::SCALE).div(to.mul(P2::SCALE)), false)
    }

    const fn by<T: Storage, T2: Storage>(m: Magnitude, rounding: bool) -> Self {
        let ratio = m.to_ratio();
        let float = T::IS_FLOAT || T2::IS_FLOAT;
        let fixed_point = if ratio.is_none() && !float && rounding {
            Some(m.fixed_point())
        } else {
            None
        };
        let (num, den) = match ratio {
            Some(r) => r,
            None if float || rounding => (1, 1),
            None => panic!(
                "an integer Fixed converts by a fraction of integers only: this factor has a power of π, ln 2 or \
                 ln 10 or a half exponent (convert through a float storage, or round with to_rounded)"
            ),
        };
        let wide = T::BITS > 32 || T2::BITS > 32;
        if !float {
            let limit = if wide {
                i128::MAX as u128
            } else {
                i64::MAX as u128
            };
            let max_abs = if T::MAX_INT.unsigned_abs() > T::MIN_INT.unsigned_abs() {
                T::MAX_INT.unsigned_abs()
            } else {
                T::MIN_INT.unsigned_abs()
            };
            assert!(
                num <= limit / max_abs,
                "the factor between these scales doesn't fit the widened multiply (64 bits for storages of up to \
                 32 bits, 128 bits else)"
            );
        }
        assert!(
            !rounding || den <= i128::MAX as u128,
            "the factor between these scales is below 2⁻¹²⁷"
        );
        let exact_f = ratio.is_some() && num <= 1 << 53 && den <= 1 << 53;
        Conversion {
            num,
            den,
            wide,
            float,
            exact_f,
            num_f: num as f64,
            den_f: den as f64,
            factor: m.to_f64(),
            fixed_point,
        }
    }

    /// The conversion of [Fixed::into_rounded] (and with `exact`, of `From` a bounded `Fixed`), with the proof that
    /// every value of `B` (or of `T` without bounds) lands in `T2`: a compile error where it doesn't.
    const fn proven<P: FixedScale, P2: FixedScale, T: Storage, T2: Storage, B: Bounds<T>>(
        exact: bool,
    ) -> Self {
        assert!(
            !T::IS_FLOAT && !T2::IS_FLOAT,
            "a proven conversion is between integer storages"
        );
        let c = Conversion::of::<P, P2, T, T2>(!exact);
        if exact {
            assert!(
                c.den == 1,
                "From a bounded Fixed takes a whole factor (more steps, never fewer): round with into_rounded"
            );
        }
        let (lo, hi) = match B::INT_RANGE {
            Some(r) => r,
            None => (T::MIN_INT, T::MAX_INT),
        };
        // the conversion is monotonic: its ends are those of the range, rounded outward
        let (lo, hi) = match c.fixed_point {
            Some((m, shift)) => (
                rounding::mul_fixed_point(lo, m, shift, Rounding::Floor),
                rounding::mul_fixed_point(hi, m, shift, Rounding::Ceil),
            ),
            None => (
                rounding::Scaled::of(rounding::div_i128(
                    lo * c.num as i128,
                    c.den as i128,
                    Rounding::Floor,
                )),
                rounding::Scaled::of(rounding::div_i128(
                    hi * c.num as i128,
                    c.den as i128,
                    Rounding::Ceil,
                )),
            ),
        };
        assert!(
            lo.at_least(T2::MIN_INT) && hi.at_most(T2::MAX_INT),
            "the bounds don't prove that every value fits the target storage: narrow the bounds or widen the storage"
        );
        c
    }

    /// `x` as a float scaled by the factor.
    #[inline(always)]
    fn scale_f64<T: Storage>(self, x: T) -> f64 {
        let x = match x.to_repr() {
            Repr::Int(i) => i as f64,
            Repr::Float(f) => f,
        };
        if self.exact_f {
            x * self.num_f / self.den_f
        } else {
            x * self.factor
        }
    }

    /// `x` in `T2`, rounded by `r` and settled by `o`; `None` only for [Overflow::Checked].
    #[inline(always)]
    fn apply_rounded<T: Storage, T2: Storage>(self, x: T, r: Rounding, o: Overflow) -> Option<T2> {
        if self.float {
            let y = self.scale_f64(x);
            return if T2::IS_FLOAT {
                T2::from_float(y)
            } else {
                rounding::settle_f64(rounding::round_f64(y, r), o)
            };
        }
        if let Some((m, shift)) = self.fixed_point {
            let Repr::Int(x) = x.to_repr() else {
                return None;
            };
            return if self.wide {
                rounding::mul_fixed_point(x, m, shift, r).settle(o)
            } else {
                rounding::mul_fixed_point_narrow(x as i64, m, shift, r).settle(o)
            };
        }
        if self.num == 1 {
            // a division only: in T's own width
            let y = x.div_rounded(self.den, r);
            return if self.wide {
                match y.to_repr() {
                    Repr::Int(i) => rounding::settle(i, o),
                    Repr::Float(_) => None,
                }
            } else {
                rounding::settle_i64(y.to_i64(), o)
            };
        }
        if !self.wide {
            // |x| · num fits i64, checked at compile time
            let p = x.to_i64() * self.num as i64;
            let y = if self.den == 1 {
                p
            } else {
                rounding::div_i64(p, self.den as i64, r)
            };
            rounding::settle_i64(y, o)
        } else {
            let Repr::Int(x) = x.to_repr() else {
                return None;
            };
            let p = x * self.num as i128;
            let y = if self.den == 1 {
                p
            } else {
                rounding::div_i128(p, i128::try_from(self.den).unwrap_or(i128::MAX), r)
            };
            rounding::settle(y, o)
        }
    }

    #[inline(always)]
    fn apply<T: Storage, T2: Storage>(self, x: T) -> Option<T2> {
        if self.float {
            return T2::from_float(self.scale_f64(x));
        }
        if self.num == 1 {
            // a division only: in T's own width
            let y = x.div_whole(self.den)?;
            return if self.wide {
                T2::from_int(match y.to_repr() {
                    Repr::Int(i) => i,
                    Repr::Float(_) => return None,
                })
            } else {
                T2::from_i64(y.to_i64())
            };
        }
        if !self.wide {
            // |x| · num fits i64, checked at compile time
            let p = x.to_i64() * self.num as i64;
            let den = self.den as i64;
            if den != 1 && p % den != 0 {
                return None;
            }
            T2::from_i64(if den == 1 { p } else { p / den })
        } else {
            let Repr::Int(x) = x.to_repr() else {
                return None;
            };
            let p = x * self.num as i128;
            let den = i128::try_from(self.den).ok()?;
            if den != 1 && p % den != 0 {
                return None;
            }
            T2::from_int(if den == 1 { p } else { p / den })
        }
    }
}

/// `x · scale` in `T`: exact for an integer `T` (`None` when not whole, out of range or with a power of π), scaled for
/// a float.
fn rescale<T: Storage>(x: Repr, scale: Magnitude) -> Option<T> {
    match x {
        Repr::Int(x) if !T::IS_FLOAT => T::from_int(scale.apply_int(x)?),
        Repr::Int(x) => T::from_float(scale.apply_f64(x as f64)),
        Repr::Float(x) => T::from_float(scale.apply_f64(x)),
    }
}

/// The number as stored and the prefix with the unit: `1500 µs`, `4 KiB`, `50 %`, a scale without a prefix after
/// the number (`15·10⁻⁴ s`, `32767·1/65535`); `{:#}` writes it in the unit itself, `0.0015 s`.
impl<U: Unit, P: FixedScale, T: Storage, B: Bounds<T>> fmt::Display for Fixed<U, P, T, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            match self.value.to_repr() {
                Repr::Int(x) => match (P::SCALE.apply_int(x), P::SCALE.as_pow10()) {
                    (Some(x), _) => write!(f, "{x}")?,
                    (None, Some(e)) => write_decimal(x, e, f)?,
                    (None, None) => write!(f, "{}", P::SCALE.apply_f64(x as f64))?,
                },
                Repr::Float(x) => write!(f, "{}", P::SCALE.apply_f64(x))?,
            }
            write_unit(Prefix::Unit, &U::EXPR, f)
        } else {
            self.value.fmt_number(f)?;
            write_prefix_unit::<P>(U::EXPR, f)
        }
    }
}

/// An integer at a power of ten below the unit, exactly: 1500 at 10⁻⁶ is `0.0015`.
pub(crate) fn write_decimal(x: i128, e: i8, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if !(-38..0).contains(&e) {
        return write!(f, "{}", Magnitude::pow10(e).apply_f64(x as f64));
    }
    let digits = e.unsigned_abs() as u32;
    let p = 10u128.pow(digits);
    let m = x.unsigned_abs();
    let (int, mut frac) = (m / p, m % p);
    let mut width = digits as usize;
    while frac % 10 == 0 {
        frac /= 10;
        width -= 1;
    }
    let sign = if x < 0 { "-" } else { "" };
    write!(f, "{sign}{int}.{frac:0width$}")
}

fn write_prefix_unit<P: FixedScale>(unit: &UnitExpr, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if let Some(prefix) = P::SCALE.as_pow10().and_then(|e| Prefix::try_from(e).ok()) {
        return write_unit(prefix, &unit, f);
    }
    if P::SYMBOL.is_empty() {
        // a factor without a prefix: after the number, then the unit
        write!(f, "{}", P::SCALE)?;
        return write_unit(Prefix::Unit, &unit, f);
    }
    match unit {
        UnitExpr::Named { symbol: "", .. } => write!(f, " {}", P::SYMBOL),
        UnitExpr::Named { symbol, .. } => write!(f, " {}{symbol}", P::SYMBOL),
        unit => write!(f, " {}({unit})", P::SYMBOL),
    }
}

/// `Second(1500 µs u32)`: the unit's name, the value as `Display` shows it, and the storage; bounds after it when
/// there are some (`Second(10 ms u16, 1..=5000)`).
impl<U: Unit, P: FixedScale, T: Storage, B: Bounds<T>> fmt::Debug for Fixed<U, P, T, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}(", U::EXPR.name())?;
        self.value.fmt_number(f)?;
        write_prefix_unit::<P>(U::EXPR, f)?;
        write!(f, " {}", T::NAME)?;
        if B::MIN.is_some() || B::MAX.is_some() {
            f.write_str(", ")?;
            if let Some(min) = B::MIN {
                min.fmt_number(f)?;
            }
            f.write_str("..=")?;
            if let Some(max) = B::MAX {
                max.fmt_number(f)?;
            }
        }
        f.write_str(")")
    }
}

/// `1500 µs`, like `Display`.
#[cfg(feature = "defmt")]
impl<U: Unit, P: FixedScale, T: Storage + defmt::Format, B: Bounds<T>> defmt::Format
    for Fixed<U, P, T, B>
{
    fn format(&self, f: defmt::Formatter<'_>) {
        match U::EXPR {
            UnitExpr::Named { symbol: "", .. } if P::RUNTIME.is_some() => {
                defmt::write!(f, "{}", self.value);
                Prefix::defmt_ratio(P::RUNTIME.unwrap_or(Prefix::Unit), f);
            }
            UnitExpr::Named { symbol, .. } if P::SYMBOL.is_empty() && P::RUNTIME.is_none() => {
                defmt::write!(f, "{} ·{} {=str}", self.value, P::SCALE, symbol)
            }
            UnitExpr::Named { symbol, .. } => {
                defmt::write!(f, "{} {=str}{=str}", self.value, P::SYMBOL, symbol)
            }
            // compound units are rare on a device: the type name instead of a rendered tree
            _ => defmt::write!(f, "{} {=str}?", self.value, P::SYMBOL),
        }
    }
}

/// A duration from a time in a prefix that a [core::time::Duration] holds exactly (s down to ns).
#[cfg(feature = "time")]
impl<P: prefix::SubSecond, T: UnsignedStorage, B: Bounds<T>>
    From<Fixed<crate::units::Second, P, T, B>> for core::time::Duration
{
    #[inline]
    fn from(t: Fixed<crate::units::Second, P, T, B>) -> Self {
        let v = t.value.to_u64();
        let per_s = P::PER_SECOND;
        core::time::Duration::new(v / per_s, ((v % per_s) * (1_000_000_000 / per_s)) as u32)
    }
}

/// A time from a duration, if it is a whole number of `P` that fits `T` and `B`.
#[cfg(feature = "time")]
impl<P: prefix::SubSecond, T: UnsignedStorage, B: Bounds<T>> TryFrom<core::time::Duration>
    for Fixed<crate::units::Second, P, T, B>
{
    type Error = OutOfBounds;

    fn try_from(d: core::time::Duration) -> Result<Self, OutOfBounds> {
        let per_s = P::PER_SECOND;
        let ns_per = 1_000_000_000 / per_s;
        let nanos = d.subsec_nanos() as u64;
        if !nanos.is_multiple_of(ns_per) {
            return Err(OutOfBounds);
        }
        let v = d
            .as_secs()
            .checked_mul(per_s)
            .and_then(|s| s.checked_add(nanos / ns_per))
            .ok_or(OutOfBounds)?;
        Self::try_new(T::from_u64(v).ok_or(OutOfBounds)?)
    }
}

#[cfg(feature = "shrink_wrap")]
/// The error type the derive's `validate` returns.
type ShrinkWrapError = shrink_wrap::Error;

#[cfg(all(test, feature = "std", feature = "shrink_wrap"))]
mod tests {
    use super::*;
    use crate::prefix::*;
    use crate::units::{Byte, Ratio, Second, Volt};
    use core::time::Duration;
    use shrink_wrap::{Nibble, UNib32, UVlq64};

    #[cfg(feature = "customary")]
    #[test]
    fn convert_between_units() {
        use crate::units::{
            GallonUs, Inch, Litre, Meter, MileInternational, Newton, Pascal, PoundForce, Psi,
            Speed, Times,
        };
        // 1 in = 25.4 mm
        let x = Fixed::<Inch, One, i32>::new(10);
        assert_eq!(x.convert::<Meter, Micro, i32>(), Some(Fixed::new(254_000)));
        assert_eq!(x.convert::<Meter, Milli, i16>(), Some(Fixed::new(254)));
        assert_eq!(
            Fixed::<Inch, One, i32>::new(1).convert::<Meter, Milli, i32>(),
            None
        );
        // and back: 254 mm is 10 in, 255 mm is not a whole number of inches
        assert_eq!(
            Fixed::<Meter, Milli, u16>::new(254).convert::<Inch, One, u8>(),
            Some(Fixed::new(10))
        );
        assert_eq!(
            Fixed::<Meter, Milli, u16>::new(255).convert::<Inch, One, u8>(),
            None
        );
        // 1 mi = 1609.344 m
        assert_eq!(
            Fixed::<MileInternational, One, u32>::new(1).convert::<Meter, Milli, u32>(),
            Some(Fixed::new(1_609_344))
        );
        // 1 gal (US) = 3.785411784 L, Litre a power of ten of m³
        assert_eq!(
            Fixed::<GallonUs, One, u32>::new(1).convert::<Litre, Pico, u64>(),
            Some(Fixed::new(3_785_411_784_000))
        );
        // psi: 6894.757293168361.. Pa, not a terminating decimal
        assert_eq!(
            Fixed::<Psi, One, u32>::new(1).convert::<Pascal, Micro, u64>(),
            None
        );
        let pa = Fixed::<Psi, One, u32>::new(1)
            .convert::<Pascal, One, f64>()
            .unwrap();
        assert!((pa.get() - 6894.757293168361).abs() < 1e-9);
        // compound units: lbf·s to N·s, and m/s to Per<Meter, Second> (the same unit)
        assert_eq!(
            Fixed::<Times<PoundForce, Second>, Milli, i64>::new(1000)
                .convert::<Times<Newton, Second>, Pow10<-13>, i64>(),
            Some(Fixed::new(44_482_216_152_605))
        );
        assert_eq!(
            Fixed::<Speed, Milli, i32>::new(-5)
                .convert::<crate::units::Per<Meter, Second>, Milli, i32>(),
            Some(Fixed::new(-5))
        );
    }

    #[test]
    fn unit_magnitude() {
        use crate::unit::{Scale, UnitExpr};
        use crate::{Dim, Magnitude};
        assert_eq!(<Volt as Unit>::EXPR.magnitude(), Some(Magnitude::ONE));
        static F: UnitExpr = UnitExpr::Named {
            name: "Affine",
            symbol: "m~",
            dim: Dim::new(0, 1, 0, 0, 0, 0, 0),
            scale: Scale::Affine {
                factor: 0.3,
                offset: 0.0,
            },
            provenance: crate::Provenance::Defined,
        };
        static M: UnitExpr = UnitExpr::Mul(&F, <Second as Unit>::EXPR);
        assert_eq!(F.magnitude(), None);
        assert_eq!(M.magnitude(), None);
    }

    #[test]
    fn exact_to() {
        let t: Micros<u32> = Fixed::new(1500);
        assert_eq!(t.to::<Milli, u16>(), None);
        assert_eq!(t.to::<Nano, u64>(), Some(Fixed::new(1_500_000)));
        assert_eq!(t.to::<Nano, u16>(), None);
        assert_eq!(
            Micros::new(-3000i32).to::<Milli, i8>(),
            Some(Fixed::new(-3))
        );
        assert_eq!(Micros::new(-3000i32).to::<Milli, u8>(), None);
        assert_eq!(t.to::<One, f32>(), Some(Fixed::new(0.0015)));
        assert_eq!(
            Fixed::<Second, One, f32>::new(0.5).to::<Milli, u16>(),
            Some(Fixed::new(500))
        );
        assert_eq!(
            Fixed::<Second, One, f32>::new(0.0005).to::<Milli, u16>(),
            None
        );
        // binary and decimal
        let kib = Fixed::<Byte, Kibi, u16>::new(4);
        assert_eq!(kib.to::<One, u32>(), Some(Fixed::new(4096)));
        assert_eq!(kib.to::<Kilo, u32>(), None);
        assert_eq!(
            Fixed::<Byte, Kilo, u32>::new(1024).to::<Kibi, u32>(),
            Some(Fixed::new(1000))
        );
        assert_eq!(
            Fixed::<Byte, Mebi, u8>::new(1).to::<Kibi, u16>(),
            Some(Fixed::new(1024))
        );
        assert_eq!(
            Fixed::<Byte, One, UVlq64>::new(UVlq64(2048)).to::<Kibi, Nibble>(),
            Some(Fixed::new(Nibble::new(2).unwrap()))
        );
    }

    #[test]
    fn display() {
        assert_eq!(format!("{}", Micros::new(1500u32)), "1500 µs");
        assert_eq!(format!("{:#}", Micros::new(1500u32)), "0.0015 s");
        assert_eq!(format!("{:#}", Micros::new(-1500i32)), "-0.0015 s");
        assert_eq!(format!("{:#}", Micros::new(3_000_000u32)), "3 s");
        assert_eq!(format!("{}", Fixed::<Byte, Kibi, u16>::new(4)), "4 KiB");
        assert_eq!(format!("{:#}", Fixed::<Byte, Kibi, u16>::new(4)), "4096 B");
        assert_eq!(format!("{}", Fixed::<Ratio, Centi, u8>::new(50)), "50 %");
        assert_eq!(format!("{}", Ppb::new(-12i32)), "-12 ppb");
        assert_eq!(format!("{}", Fixed::<Volt, Kilo, f32>::new(1.5)), "1.5 kV");
        assert_eq!(format!("{}", Bytes::new(UNib32(7))), "7 B");
        assert_eq!(format!("{:?}", Micros::new(1500u32)), "Second(1500 µs u32)");
        type T = Fixed<Second, Milli, u16, Range<1, 5000>>;
        assert_eq!(
            format!("{:?}", T::try_new(10).unwrap()),
            "Second(10 ms u16, 1..=5000)"
        );
    }

    #[test]
    fn bounds() {
        type T = Fixed<Second, Milli, u16, Range<1, 5000>>;
        assert_eq!(T::try_new(0), Err(OutOfBounds));
        assert_eq!(T::try_new(5001), Err(OutOfBounds));
        let t = T::try_new(4000).unwrap();
        assert_eq!(t.checked_add(t), None);
        assert_eq!(t.saturating_add(t).get(), 5000);
        assert_eq!(T::bounded(1).saturating_sub(t).get(), 1);
        type F = Fixed<Ratio, One, f32, RangeDec<-5, 25, -1>>;
        assert!(F::try_new(2.5).is_ok());
        assert!(F::try_new(-0.6).is_err());
        assert!(F::try_new(f32::NAN).is_err());
        type W = Fixed<Byte, One, u64, RangeU64<0, { u64::MAX - 1 }>>;
        assert!(W::try_new(u64::MAX).is_err());
        // decode is checked
        let bytes = Micros::new(6000u16)
            .to_ww_bytes(&mut [0u8; 8])
            .unwrap()
            .to_vec();
        assert_eq!(
            T::from_ww_bytes(&bytes),
            Err(shrink_wrap::Error::SubtypeOutOfRange)
        );
        assert_eq!(
            T::from_ww_bytes(Millis::new(10u16).to_ww_bytes(&mut [0u8; 8]).unwrap())
                .unwrap()
                .get(),
            10
        );
    }

    #[test]
    fn ops() {
        let a: Micros<u32> = Fixed::new(10);
        assert_eq!(a + a, Fixed::new(20));
        assert_eq!(a - Fixed::new(4), Fixed::new(6));
        assert_eq!(a * 3, Fixed::new(30));
        assert_eq!(a / 3, Fixed::new(3));
        assert_eq!([a, a, a].into_iter().sum::<Micros<u32>>(), Fixed::new(30));
        assert_eq!(
            Micros::new(u32::MAX).wrapping_add(Fixed::new(2)),
            Fixed::new(1)
        );
        assert_eq!(Micros::new(u32::MAX).checked_add(Fixed::new(2)), None);
        assert_eq!(
            Micros::new(u32::MAX).saturating_add(Fixed::new(2)),
            Fixed::new(u32::MAX)
        );
        let n = Fixed::<Byte, One, Nibble>::new(Nibble::new(14).unwrap());
        assert_eq!(n.checked_add(n), None);
        assert_eq!(n.wrapping_add(n).get().value(), 12);
        assert_eq!(n.saturating_add(n).get().value(), 15);
        assert_eq!(
            Bytes::new(UNib32(3)) + Bytes::new(UNib32(4)),
            Bytes::new(UNib32(7))
        );
        assert!(a < a * 2);
    }

    #[test]
    fn duration() {
        assert_eq!(
            Duration::from(Micros::new(1_500_001u64)),
            Duration::new(1, 500_001_000)
        );
        assert_eq!(
            Duration::from(Fixed::<Second, One, u8>::new(3)),
            Duration::from_secs(3)
        );
        assert_eq!(
            Duration::from(Nanos::new(UVlq64(7))),
            Duration::from_nanos(7)
        );
        assert_eq!(
            Millis::<u16>::try_from(Duration::from_millis(1500)),
            Ok(Fixed::new(1500))
        );
        assert_eq!(
            Millis::<u16>::try_from(Duration::from_micros(1500)),
            Err(OutOfBounds)
        );
        assert_eq!(
            Millis::<u16>::try_from(Duration::from_secs(70)),
            Err(OutOfBounds)
        );
    }

    #[test]
    fn scales_without_a_prefix() {
        // tenths of a ms
        let t = Fixed::<Second, Pow10<-4>, u32>::new(15);
        assert_eq!(format!("{t}"), "15·10⁻⁴ s");
        assert_eq!(format!("{t:#}"), "0.0015 s");
        assert_eq!(t.to::<Micro, u32>(), Some(Fixed::new(1500)));
        assert_eq!(t.to::<Milli, u32>(), None);
        assert_eq!(Fixed::<Second, Pow10<-3>, u8>::new(7).to_string(), "7 ms");
        assert_eq!(
            Fixed::<Second, Pow10<-4>, u32>::from_scaled(Repr::Int(300), Magnitude::pow10(-6)),
            Some(Fixed::new(3))
        );
        assert_eq!(
            Fixed::<Second, Pow10<-4>, u32>::from_scaled(Repr::Int(301), Magnitude::pow10(-6)),
            None
        );
        // a 16-bit duty
        let half = Duty16::new(32767);
        assert_eq!(format!("{half}"), "32767·1/65535");
        assert_eq!(Duty16::new(65535).to::<One, u8>(), Some(Fixed::new(1)));
        assert_eq!(half.to::<Centi, u8>(), None);
        assert_eq!(Duty16::new(13107).to::<Centi, u8>(), Some(Fixed::new(20)));
        assert!((half.to::<One, f32>().unwrap().get() - 0.5).abs() < 1e-4);
        // a binary angle: 65536 is a turn
        let a = Fixed::<crate::units::Turn, Rational<1, 65536>, u16>::new(16384);
        assert_eq!(format!("{a:#}"), "0.25 rev");
        assert_eq!(a.to::<Rational<1, 4>, u8>(), Some(Fixed::new(1)));
    }

    #[test]
    fn exact_angles() {
        use crate::units::{Degree, Turn};
        let deg: &crate::UnitExpr = <Degree as crate::Unit>::EXPR;
        assert!((deg.factor() - core::f64::consts::PI / 180.0).abs() < 1e-18);
        assert!((<Turn as crate::Unit>::EXPR.factor() - core::f64::consts::TAU).abs() < 1e-15);
    }

    #[test]
    fn cycles_are_hertz_by_name() {
        use crate::units::{Cycle, Per};
        let g = Fixed::<Per<Cycle, Second>, Kilo, u16>::new(32);
        assert_eq!(
            g.to_hertz(),
            Fixed::<crate::units::Hertz, Kilo, u16>::new(32)
        );
    }

    #[test]
    fn wire_is_the_number() {
        let mut buf = [0u8; 16];
        let t: Micros<u64> = Fixed::new(0x0102_0304_0506_0708);
        assert_eq!(
            t.to_ww_bytes(&mut buf).unwrap(),
            0x0102_0304_0506_0708u64
                .to_ww_bytes(&mut [0u8; 16])
                .unwrap()
        );
        assert_eq!(
            Micros::<u64>::from_ww_bytes(
                0x0102_0304_0506_0708u64
                    .to_ww_bytes(&mut [0u8; 16])
                    .unwrap()
            )
            .unwrap(),
            t
        );
        assert_eq!(core::mem::size_of::<Micros<u64>>(), 8);
    }

    #[test]
    fn scales_equal_however_spelled() {
        assert_eq!(Milli::SCALE, Pow10::<-3>::SCALE);
        assert_eq!(Milli::SCALE, Rational::<1, 1000>::SCALE);
        assert_eq!(Kibi::SCALE, Pow2::<10>::SCALE);
        assert_eq!(Duty16::new(0).get(), 0);
        assert_eq!(FullScaleMax::<One, 16>::SCALE, Rational::<1, 65535>::SCALE);
        // equal scales convert as the plain number
        let t = Fixed::<Second, Milli, u32>::new(1234);
        assert_eq!(t.to::<Pow10<-3>, u32>().map(Fixed::get), Some(1234));
        assert_eq!(t.to::<Rational<1, 1000>, u16>().map(Fixed::get), Some(1234));
        assert_eq!(
            Fixed::<Second, Milli, u32>::new(70_000).to::<Milli, u16>(),
            None
        );
    }

    #[test]
    fn q_formats_and_full_scale() {
        // Q15: 0.5 is 16384; in Pow2<-14> steps 8192
        let half: Q15 = Fixed::of(16384);
        assert_eq!(half.to::<Pow2<-14>, i16>().map(Fixed::get), Some(8192));
        assert_eq!(half.to::<One, f32>().map(Fixed::get), Some(0.5));
        assert_eq!(half.to::<Pow2<-16>, i16>(), None, "131072 doesn't fit i16");
        let q31: Q31 = Fixed::of(-(1 << 30));
        assert_eq!(q31.to::<Pow2<-15>, i16>().map(Fixed::get), Some(-16384));
        let uq: UQ16_16 = Fixed::of(3 << 16);
        assert_eq!(uq.to::<One, u8>().map(Fixed::get), Some(3));
        // a 12-bit ADC at 3.3 V: 4096 codes are 3.3 V, 1241 codes 999.8 mV (not whole), 1000 codes 805.6 mV
        type Adc = Fixed<Volt, FullScale<Rational<33, 10>, 12>, u16>;
        assert_eq!(
            Adc::new(4096).to::<Milli, u16>().map(Fixed::get),
            Some(3300)
        );
        assert_eq!(Adc::new(1241).to::<Milli, u16>(), None);
        assert_eq!(
            Adc::new(2048).to::<Micro, u32>().map(Fixed::get),
            Some(1_650_000)
        );
        let mv = Adc::new(1000).to::<Milli, f32>().unwrap().get();
        assert!((mv - 805.664_06).abs() < 1e-3);
    }

    #[cfg(feature = "angle")]
    #[test]
    fn binary_angles() {
        use crate::units::{Degree, Turn};
        let quarter: Bam16 = Fixed::of(16384);
        assert_eq!(quarter.to::<Pow2<-2>, u8>().map(Fixed::get), Some(1));
        assert_eq!(
            Bam32::of(1 << 30).to::<Pow2<-16>, u16>().map(Fixed::get),
            Some(16384)
        );
        // wraps at a turn: 3/4 turn + 1/2 turn is 1/4 turn, by `+` itself
        let x = Bam16::of(49152) + Bam16::of(32768);
        assert_eq!(x.get(), 16384);
        assert_eq!(Bam16::of(49152).wrapping_add(Bam16::of(32768)), x);
        assert_eq!(
            format!("{}", Fixed::<Turn, Pow2<-16>, u16>::new(1)),
            "1·2⁻¹⁶ rev"
        );
        // a turn is 360 °: only through the unit's own scale, Turn and Degree are different markers
        assert_eq!(
            <Turn as crate::Unit>::EXPR.factor() / <Degree as crate::Unit>::EXPR.factor(),
            360.0
        );
    }

    #[test]
    fn wide_storages() {
        // u64 ns to u64 ps multiplies in 128 bits
        let t = Fixed::<Second, Nano, u64>::new(u64::MAX / 1000);
        assert_eq!(
            t.to::<Pico, u64>().map(Fixed::get),
            Some(u64::MAX / 1000 * 1000)
        );
        assert_eq!(
            Fixed::<Second, Nano, u64>::new(u64::MAX).to::<Pico, u64>(),
            None
        );
        assert_eq!(
            Fixed::<Second, Nano, i64>::new(-3_000_000)
                .to::<Milli, i8>()
                .map(Fixed::get),
            Some(-3)
        );
        // u32 seconds to u64 ns: 10⁹ · u32::MAX fits 128 bits
        assert_eq!(
            Fixed::<Second, One, u32>::new(u32::MAX)
                .to::<Nano, u64>()
                .map(Fixed::get),
            Some(u32::MAX as u64 * 1_000_000_000)
        );
        // a large divisor in a narrow type: only 0 is whole
        assert_eq!(
            Fixed::<Second, Pico, u8>::new(0)
                .to::<Kilo, u8>()
                .map(Fixed::get),
            Some(0)
        );
        assert_eq!(Fixed::<Second, Pico, u8>::new(5).to::<Kilo, u8>(), None);
    }
}

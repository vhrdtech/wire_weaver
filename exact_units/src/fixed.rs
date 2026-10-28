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

pub use bounds::{Bounds, OutOfBounds, Range, RangeDec, RangeU64, Unbounded};
pub use prefix::{DecimalPrefix, FixedScale};
pub use storage::{Repr, Storage, UnsignedStorage};

mod bounds;
pub mod prefix;
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

impl<U, P, T: Storage + Default> Default for Fixed<U, P, T> {
    fn default() -> Self {
        Self::new(T::default())
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
    /// The same value in prefix `P2` and storage `T2`, if that is exact: `None` when it is not a whole number of
    /// `P2` for an integer `T2`, or doesn't fit `T2`. Floats are scaled.
    pub fn to<P2: FixedScale, T2: Storage>(self) -> Option<Fixed<U, P2, T2>> {
        rescale(self.value.to_repr(), P::SCALE.div(P2::SCALE)).map(Fixed::new)
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

impl<U, P, T: Storage> Fixed<U, P, T> {
    /// `self + rhs`, wrapping around at the edge of `T` like a counter: `u32::MAX` µs + 2 µs is 1 µs. Only without
    /// bounds.
    #[inline(always)]
    pub fn wrapping_add(self, rhs: Self) -> Self {
        Self::new(self.value.wrapping_add(rhs.value))
    }

    /// `self - rhs`, wrapping around at the edge of `T`. Only without bounds.
    #[inline(always)]
    pub fn wrapping_sub(self, rhs: Self) -> Self {
        Self::new(self.value.wrapping_sub(rhs.value))
    }

    /// `self * n`, wrapping around at the edge of `T`. Only without bounds.
    #[inline(always)]
    pub fn wrapping_mul(self, n: T) -> Self {
        Self::new(self.value.wrapping_mul(n))
    }
}

impl<U, P, T: Storage, B: Bounds<T>> Clone for Fixed<U, P, T, B> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<U, P, T: Storage, B: Bounds<T>> Copy for Fixed<U, P, T, B> {}

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

/// `+` and `-` of the same type, `*` and `/` by a number: like the same operator on `T` (panics on overflow in debug
/// builds). Only without bounds; a bounded value has [checked_add](Fixed::checked_add) and friends.
mod arith {
    use super::*;
    use core::iter::Sum;
    use core::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Sub, SubAssign};

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
fn write_decimal(x: i128, e: i8, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
}

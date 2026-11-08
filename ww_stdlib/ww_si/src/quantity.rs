//! Constructors, conversions, comparisons and formatting, written once for every [Quantity].

use crate::scale::{ToIn, compare, scale_value};
use crate::tree::{self, write_unit};
use crate::{OutOfBounds, Overflow, Prefix, Quantity, QuantityBounds, Rounding};
use core::cmp::Ordering;
use core::fmt;
use core::marker::PhantomData;
use exact_units::{Unit, UnitExpr};
use ww_numeric::{Integer, Magnitude, NumericBaseType, NumericValue};

impl<U: Unit> Quantity<U> {
    /// A quantity of `value` in units of `prefix`, in `const` code (statics, tables).
    pub const fn from_parts(prefix: Prefix, value: NumericValue) -> Self {
        Quantity {
            prefix,
            value,
            _unit: PhantomData,
            _bounds: PhantomData,
        }
    }

    /// A quantity of `value` (any number type that is enabled: `u16`, `f32`, `UVlq32`, ..) in units of `prefix`.
    pub fn new(prefix: Prefix, value: impl Into<NumericValue>) -> Self {
        Self::from_parts(prefix, value.into())
    }

    /// `value` in the unit itself, no prefix: `Volt::base(3u8)` is 3 V.
    pub fn base(value: impl Into<NumericValue>) -> Self {
        Self::new(Prefix::Unit, value)
    }

    /// `value` in milli-units: `Volt::milli(3300u16)` is 3300 mV.
    pub fn milli(value: impl Into<NumericValue>) -> Self {
        Self::new(Prefix::Milli, value)
    }

    /// `value` in micro-units: `Second::micro(100u32)` is 100 µs.
    pub fn micro(value: impl Into<NumericValue>) -> Self {
        Self::new(Prefix::Micro, value)
    }

    /// `value` in nano-units.
    pub fn nano(value: impl Into<NumericValue>) -> Self {
        Self::new(Prefix::Nano, value)
    }

    /// `value` in pico-units.
    pub fn pico(value: impl Into<NumericValue>) -> Self {
        Self::new(Prefix::Pico, value)
    }

    /// `value` in centi-units: `Ratio::centi(50u8)` is 50 %.
    pub fn centi(value: impl Into<NumericValue>) -> Self {
        Self::new(Prefix::Centi, value)
    }

    /// `value` in kilo-units: `Hertz::kilo(400u32)` is 400 kHz, `Gram::kilo(5u8)` 5 kg.
    pub fn kilo(value: impl Into<NumericValue>) -> Self {
        Self::new(Prefix::Kilo, value)
    }

    /// `value` in mega-units.
    pub fn mega(value: impl Into<NumericValue>) -> Self {
        Self::new(Prefix::Mega, value)
    }

    /// `value` in giga-units.
    pub fn giga(value: impl Into<NumericValue>) -> Self {
        Self::new(Prefix::Giga, value)
    }

    /// `value` in the unit itself, stored as `f32`.
    #[cfg(feature = "f32")]
    pub const fn from_f32(value: f32) -> Self {
        Self::from_parts(Prefix::Unit, NumericValue::F32(value))
    }

    /// `x` (in the unit itself) in units of `prefix` and stored as `storage`: rounded to the nearest integer and
    /// saturated at the storage's range, as is for a float storage. What a device does to send a float it computed
    /// in the representation its API documents: `Ampere::quantize(0.1523, Prefix::Milli, NumericBaseType::I32)` is
    /// 152 mA. Computes in `f32`, no `f64` code on a device.
    ///
    /// Panics if `storage` is not enabled as a number feature, or is one `NumericValue` can't hold (`F16`, `UB`, ..).
    pub fn quantize(x: f32, prefix: Prefix, storage: NumericBaseType) -> Self {
        let x = Prefix::Unit
            .ratio_to(prefix)
            .map_or(f32::NAN, |r| r.apply_f32(x));
        let value = storage
            .from_f32_saturating(x)
            .expect("quantize: number type not enabled or not supported");
        Self::from_parts(prefix, value)
    }
}

/// Checked constructors and everything that reads a value, for a quantity with any bounds.
impl<U: Unit, B: QuantityBounds> Quantity<U, B> {
    /// `value` in units of `prefix`, [OutOfBounds] when it is outside `B` (in the unit itself, whatever `prefix`):
    /// `Quantity::<units::Volt, RangeDec<0, 33, -1>>::try_new(Prefix::Milli, 3300u16)` is 3.3 V, 3301 mV is an
    /// error. Never fails for [Unbounded](crate::Unbounded), whose plain constructors ([new](Quantity::new),
    /// [milli](Quantity::milli), ..) need no check.
    pub fn try_new(prefix: Prefix, value: impl Into<NumericValue>) -> Result<Self, OutOfBounds> {
        let value = value.into();
        B::check(prefix, &value)?;
        Ok(Self::from_parts_unchecked(prefix, value))
    }

    /// The same value with the bounds `B2`, [OutOfBounds] when it is outside them: from a plain [Quantity] into a
    /// bounded one, or back with `B2 = Unbounded` (or [unbounded](Self::unbounded), which can't fail).
    pub fn bounded<B2: QuantityBounds>(self) -> Result<Quantity<U, B2>, OutOfBounds> {
        Quantity::try_new(self.prefix, self.value)
    }

    /// The same value without bounds.
    pub const fn unbounded(self) -> Quantity<U> {
        Quantity::from_parts(self.prefix, self.value)
    }

    const fn from_parts_unchecked(prefix: Prefix, value: NumericValue) -> Self {
        Quantity {
            prefix,
            value,
            _unit: PhantomData,
            _bounds: PhantomData,
        }
    }

    /// Run on every value read from the wire: a value outside `B` is [shrink_wrap::Error::SubtypeOutOfRange].
    #[inline(always)]
    pub(crate) fn validate(&self) -> Result<(), shrink_wrap::Error> {
        B::check(self.prefix, &self.value).map_err(|_| shrink_wrap::Error::SubtypeOutOfRange)
    }

    /// The value in the unit itself (no prefix) as `f64`, rounded where `f64` can't hold it exactly. For a unit with a
    /// scale (°, °C, g) this is still in that unit, [to_si_f64](Self::to_si_f64) converts to coherent SI.
    pub fn to_f64(&self) -> f64 {
        self.prefix.scale_f64(self.value.to_f64())
    }

    /// The value in the unit itself (no prefix) as `f32`.
    pub fn to_f32(&self) -> f32 {
        self.prefix.scale_f32(self.value.as_f32())
    }

    /// The value in units of `prefix` as `f64` (`to_f64_in(Prefix::Milli)` of 3.3 V is 3300.0): the `f64` nearest
    /// the exact value, NaN for [Prefix::Unknown].
    pub fn to_f64_in(&self, prefix: Prefix) -> f64 {
        self.prefix
            .ratio_to(prefix)
            .map_or(f64::NAN, |r| r.apply_f64(self.value.to_f64()))
    }

    /// The value in units of `prefix` as a `T` (a primitive integer, or `f32` / `f64` with those features): rounded by
    /// `r` where it isn't a whole number of `prefix`, settled into `T` by `o`. Exact for integer storage whatever the
    /// prefixes (SI, binary, `Pow10`, `Pow2`, `FullScale`): one rounding of the exact value, in integers at the width
    /// of the enabled number types (`u32` with types up to 32 bits), no float and no 128-bit code. A float value is
    /// scaled to the nearest float first, then rounded by `r`, in integers too. `None` for [Prefix::Unknown], NaN,
    /// `UN` / `IN`, and where `o` is [Overflow::Checked] and the value doesn't fit `T`.
    ///
    /// ```
    /// use ww_si::{Overflow, Prefix, Rounding, Second, Volt};
    ///
    /// // 3999 mV in volts: toward zero, to nearest
    /// let v = Volt::milli(3999u16);
    /// assert_eq!(v.to_in::<u32>(Prefix::Unit, Rounding::Truncate, Overflow::Checked), Some(3));
    /// assert_eq!(v.to_in::<u32>(Prefix::Unit, Rounding::NearestEven, Overflow::Checked), Some(4));
    /// // a timestamp against a wrapping 32-bit µs counter: 5000 s is past u32::MAX µs
    /// let t = Second::base(5_000u32);
    /// assert_eq!(t.to_in::<u32>(Prefix::Micro, Rounding::Truncate, Overflow::Wrap), Some(705_032_704));
    /// assert_eq!(t.to_in::<u32>(Prefix::Micro, Rounding::Truncate, Overflow::Checked), None);
    /// assert_eq!(t.to_in::<u32>(Prefix::Micro, Rounding::Truncate, Overflow::Saturate), Some(u32::MAX));
    /// ```
    #[inline]
    pub fn to_in<T: ToIn>(&self, prefix: Prefix, r: Rounding, o: Overflow) -> Option<T> {
        T::of_value(&self.value, self.prefix, prefix, r, o)
    }

    /// The value in coherent SI through the unit's scale: 90 ° is 1.5708 (rad), 5 g is 0.005
    /// (kg). Equal to [to_f64](Self::to_f64) for a coherent unit (V, A, N·m).
    pub fn to_si_f64(&self) -> f64 {
        self.to_f64() * U::EXPR.factor()
    }

    /// The same value in units of `prefix` and in the same number type: rounded by `r` where it isn't a whole number
    /// of `prefix`, settled into the number type by `o` (wrapping at its width, saturating at its range), exactly as
    /// [to_in](Self::to_in) does. Floats are scaled to the nearest float, `r` and `o` are for integers. `None` where
    /// `to_in` is, and for a result outside `B`.
    ///
    /// ```
    /// use ww_si::{Overflow, Prefix, Rounding, Volt};
    ///
    /// let r = Rounding::NearestEven;
    /// assert_eq!(Volt::milli(3300u16).rescaled(Prefix::Unit, r, Overflow::Checked), Some(Volt::base(3u16)));
    /// // 3 300 000 µV doesn't fit a U16
    /// assert_eq!(Volt::milli(3300u16).rescaled(Prefix::Micro, r, Overflow::Checked), None);
    /// assert_eq!(Volt::milli(3300u16).rescaled(Prefix::Micro, r, Overflow::Saturate), Some(Volt::micro(u16::MAX)));
    /// ```
    pub fn rescaled(&self, prefix: Prefix, r: Rounding, o: Overflow) -> Option<Self> {
        let value = match self.value {
            #[cfg(feature = "f32")]
            NumericValue::F32(x) => NumericValue::F32(self.prefix.ratio_to(prefix)?.apply_f32(x)),
            #[cfg(feature = "f64")]
            NumericValue::F64(x) => NumericValue::F64(self.prefix.ratio_to(prefix)?.apply_f64(x)),
            value => {
                let ty = value.ty();
                let s =
                    scale_value::<Magnitude>(&value, self.prefix, prefix, r, o == Overflow::Wrap)?;
                let x = Integer::new(s.neg, s.mag);
                match (s.overflowed, o) {
                    (false, _) => match ty.from_integer(x) {
                        Some(v) => v,
                        None if o == Overflow::Checked => return None,
                        None if o == Overflow::Saturate => ty.from_integer_saturating(x)?,
                        None => ty.from_integer_wrapping(x)?,
                    },
                    (true, Overflow::Checked) => return None,
                    (true, Overflow::Saturate) => {
                        ty.from_integer_saturating(Integer::new(s.neg, Magnitude::MAX))?
                    }
                    (true, Overflow::Wrap) => ty.from_integer_wrapping(x)?,
                }
            }
        };
        B::contains(prefix, &value).then_some(Self::from_parts_unchecked(prefix, value))
    }

    /// The same value in units of `prefix` and in the same number type, if that is exact: `Volt::milli(3000u16)` to
    /// `Prefix::Unit` is 3 V, 3300 mV is `None` (3.3 is not an integer), and so is 3300 mV to µV (3 300 000 doesn't fit
    /// a `U16`). Floats are scaled to the nearest float. For autoranging a value before sending it.
    pub fn rescaled_exact(&self, prefix: Prefix) -> Option<Self> {
        let value = match self.value.to_integer() {
            Some(x) => self.value.ty().from_integer(crate::scale::exact_between(
                x,
                self.prefix,
                prefix,
            )?)?,
            None => return self.rescaled(prefix, Rounding::NearestEven, Overflow::Checked),
        };
        B::contains(prefix, &value).then_some(Self::from_parts_unchecked(prefix, value))
    }

    /// Same prefix and same number in the same type: cheaper than `==`, which compares values across prefixes and
    /// types. For "did this change since I last sent it".
    pub fn same_repr(&self, other: &Self) -> bool {
        self.prefix == other.prefix && self.value == other.value
    }

    /// The unit: name, symbol, dimension, scale.
    pub fn unit() -> &'static UnitExpr {
        U::EXPR
    }
}

/// Equal when the values are, whatever prefix and number type each one is stored in (`1000 mV == 1 V`).
impl<U: Unit, B: QuantityBounds> PartialEq for Quantity<U, B> {
    fn eq(&self, other: &Self) -> bool {
        self.partial_cmp(other) == Some(Ordering::Equal)
    }
}

/// Compares the values across prefixes and number types: exactly when both are integers, whatever the prefixes (no
/// overflow even 60 orders of magnitude apart, and no wider arithmetic than the enabled number types need: `u32` with
/// types up to 32 bits), and an `f32` against an integer; two `f32`s through the scaled float, an `f64` through `f64`.
impl<U: Unit, B: QuantityBounds> PartialOrd for Quantity<U, B> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        compare(self.prefix, &self.value, other.prefix, &other.value)
    }
}

/// `3300 mV`, the number as stored with its prefix; `{:#}` writes it in the unit itself: `3.3 V`.
impl<U: Unit, B: QuantityBounds> fmt::Display for Quantity<U, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&tree::quantity(self.prefix, &self.value, &U::EXPR), f)
    }
}

/// `Volt(3300 mV U16)`: the unit's name, the value as `Display` shows it, and the number type.
impl<U: Unit, B: QuantityBounds> fmt::Debug for Quantity<U, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}(", U::EXPR.name())?;
        tree::write_number(&self.value, f)?;
        write_unit(self.prefix, &U::EXPR, f)?;
        write!(f, " {:?})", self.value.ty())
    }
}

/// `3300 mV`, like `Display`.
#[cfg(feature = "defmt")]
impl<U: Unit, B: QuantityBounds> defmt::Format for Quantity<U, B> {
    fn format(&self, f: defmt::Formatter<'_>) {
        defmt_number(&self.value, f);
        let unit = U::EXPR;
        if let UnitExpr::Named(def) = unit {
            let symbol = def.names.symbol;
            if symbol.is_empty() {
                Prefix::defmt_ratio(self.prefix, f);
            } else {
                defmt::write!(f, " {=str}{=str}", self.prefix.symbol(), symbol);
            }
        } else {
            // compound units are rare on a device: the type name instead of a rendered tree
            defmt::write!(f, " {=str}{}", self.prefix.symbol(), DefmtName(unit));
        }
    }
}

/// The number as stored, for `defmt`.
#[cfg(feature = "defmt")]
pub(crate) fn defmt_number(value: &NumericValue, f: defmt::Formatter<'_>) {
    match *value {
        #[cfg(feature = "f32")]
        NumericValue::F32(x) => defmt::write!(f, "{=f32}", x),
        #[cfg(feature = "f64")]
        NumericValue::F64(x) => defmt::write!(f, "{=f64}", x),
        value => match value.to_integer() {
            Some(x) => defmt::write!(f, "{}", x),
            None => defmt::write!(f, "?"),
        },
    }
}

#[cfg(feature = "defmt")]
struct DefmtName(&'static UnitExpr);

#[cfg(feature = "defmt")]
impl defmt::Format for DefmtName {
    fn format(&self, f: defmt::Formatter<'_>) {
        match self.0 {
            UnitExpr::Named(def) => defmt::write!(f, "{=str}", def.names.symbol),
            UnitExpr::Mul(a, b) => defmt::write!(f, "({}·{})", DefmtName(a), DefmtName(b)),
            UnitExpr::Div(a, b) => defmt::write!(f, "({}/{})", DefmtName(a), DefmtName(b)),
            UnitExpr::Pow(a, n) => defmt::write!(f, "({}^{=i8})", DefmtName(a), n),
            UnitExpr::Sqrt(a) => defmt::write!(f, "√({})", DefmtName(a)),
        }
    }
}

/// Cycles per second are hertz: the counted [Cycle](crate::units::Cycle) and the hertz of a clock are different dimensions (Hz is s⁻¹),
/// so going from one to the other is this named step, never an implicit conversion.
#[cfg(all(feature = "counting", feature = "time"))]
impl crate::Quantity<crate::units::Per<crate::units::Cycle, crate::units::Second>> {
    /// The same number and prefix in Hz: 8 Mcycle/s is 8 MHz.
    pub const fn to_hertz(self) -> crate::Hertz {
        crate::Quantity::from_parts(self.prefix, self.value)
    }

    /// The same number and prefix in cycles per second.
    pub const fn from_hertz(f: crate::Hertz) -> Self {
        crate::Quantity::from_parts(f.prefix, f.value)
    }
}

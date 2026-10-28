//! Constructors, conversions, comparisons and formatting, written once for every [Quantity].

use crate::ops::{
    compare, exact_between, scale_f32_between, scale_f64_between, to_i32_scaled, to_u32_scaled,
    to_u32_wrapping_scaled,
};
use crate::tree::{self, write_unit};
use crate::{Prefix, Quantity};
use core::cmp::Ordering;
use core::fmt;
use core::marker::PhantomData;
use exact_units::{Unit, UnitExpr};
use ww_numeric::{NumericBaseType, NumericValue};

impl<U: Unit> Quantity<U> {
    /// A quantity of `value` in units of `prefix`, in `const` code (statics, tables).
    pub const fn from_parts(prefix: Prefix, value: NumericValue) -> Self {
        Quantity {
            prefix,
            value,
            _unit: PhantomData,
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
        let x = scale_f32_between(x, Prefix::Unit, prefix);
        let value = storage
            .from_f32_saturating(x)
            .expect("quantize: number type not enabled or not supported");
        Self::from_parts(prefix, value)
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

    /// The value in units of `prefix` as `f64` (`to_f64_in(Prefix::Milli)` of 3.3 V is 3300.0).
    pub fn to_f64_in(&self, prefix: Prefix) -> f64 {
        scale_f64_between(self.value.to_f64(), self.prefix, prefix)
    }

    /// The value in units of `prefix` as an `i32`: exact for integer storage (truncated toward zero when `prefix` is
    /// bigger than the stored one), rounded for floats (`f32` arithmetic for an `F32`). `None` when it doesn't fit or is
    /// `NaN`. `u32` arithmetic only with number types up to 32 bits: `Second::milli(5u16).to_i32_in(Prefix::Micro)` is
    /// `Some(5000)`.
    pub fn to_i32_in(&self, prefix: Prefix) -> Option<i32> {
        to_i32_scaled(&self.value, self.prefix, prefix)
    }

    /// The value in units of `prefix` as a `u32`, like [to_i32_in](Self::to_i32_in): `None` below zero.
    pub fn to_u32_in(&self, prefix: Prefix) -> Option<u32> {
        to_u32_scaled(&self.value, self.prefix, prefix)
    }

    /// The value in units of `prefix` modulo `2^32` (two's complement below zero): what a wrapping 32-bit counter of
    /// that unit shows, for comparing a timestamp with a device clock (`Second::micro(u64)` past `u32::MAX` wraps).
    /// Truncated toward zero when `prefix` is bigger than the stored one. Floats are rounded and taken from `-2^31`
    /// up to below `2^32` only, `None` beyond, for `NaN` and `UN` / `IN`.
    pub fn to_u32_wrapping_in(&self, prefix: Prefix) -> Option<u32> {
        to_u32_wrapping_scaled(&self.value, self.prefix, prefix)
    }

    /// The value in units of `prefix` as an integer: exact for integer storage (truncated toward zero when `prefix` is
    /// bigger than the stored one), rounded for floats. `None` when it doesn't fit in `i128` or is `NaN`. Only with a
    /// 64- or 128-bit number type enabled (`u64`, `i64`, `uvlq64`, `u128`, `i128`, or `std`): with types up to 32 bits,
    /// [to_i32_in](Self::to_i32_in) and [to_u32_in](Self::to_u32_in) do it without 128-bit code.
    #[cfg(any(
        feature = "u64",
        feature = "i64",
        feature = "uvlq64",
        feature = "u128",
        feature = "i128"
    ))]
    pub fn to_i128_in(&self, prefix: Prefix) -> Option<i128> {
        crate::ops::to_i128_scaled(&self.value, self.prefix, prefix)
    }

    /// The value in coherent SI through the unit's scale: 90 ° is 1.5708 (rad), 20 °C is 293.15 (K), 5 g is 0.005
    /// (kg). Equal to [to_f64](Self::to_f64) for a coherent unit (V, A, N·m).
    pub fn to_si_f64(&self) -> f64 {
        self.to_f64() * U::EXPR.factor() + U::EXPR.offset()
    }

    /// The same value in units of `prefix` and in the same number type, if that is exact: `Volt::milli(3000u16)` to
    /// `Prefix::Unit` is 3 V, 3300 mV is `None` (3.3 is not an integer), and so is 3300 mV to µV (3 300 000 doesn't fit
    /// a `U16`). Floats are scaled. For autoranging a value before sending it.
    pub fn with_prefix(&self, prefix: Prefix) -> Option<Self> {
        let (from, to) = (self.prefix, prefix);
        let value = match self.value {
            #[cfg(feature = "f32")]
            NumericValue::F32(x) => NumericValue::F32(scale_f32_between(x, from, to)),
            #[cfg(feature = "f64")]
            NumericValue::F64(x) => NumericValue::F64(scale_f64_between(x, from, to)),
            value => {
                let x = exact_between(value.to_integer()?, from, to)?;
                value.ty().from_integer(x)?
            }
        };
        Some(Self::from_parts(prefix, value))
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
impl<U: Unit> PartialEq for Quantity<U> {
    fn eq(&self, other: &Self) -> bool {
        self.partial_cmp(other) == Some(Ordering::Equal)
    }
}

/// Compares the values across prefixes and number types: exactly when both are integers (no overflow even 60 orders
/// of magnitude apart, and no wider arithmetic than the enabled number types need: `u32` with types up to 32 bits),
/// through `f64` when one is a float.
impl<U: Unit> PartialOrd for Quantity<U> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        compare(self.prefix, &self.value, other.prefix, &other.value)
    }
}

/// `3300 mV`, the number as stored with its prefix; `{:#}` writes it in the unit itself: `3.3 V`.
impl<U: Unit> fmt::Display for Quantity<U> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&tree::quantity(self.prefix, &self.value, &U::EXPR), f)
    }
}

/// `Volt(3300 mV U16)`: the unit's name, the value as `Display` shows it, and the number type.
impl<U: Unit> fmt::Debug for Quantity<U> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}(", U::EXPR.name())?;
        tree::write_number(&self.value, f)?;
        write_unit(self.prefix, &U::EXPR, f)?;
        write!(f, " {:?})", self.value.ty())
    }
}

/// `3300 mV`, like `Display`.
#[cfg(feature = "defmt")]
impl<U: Unit> defmt::Format for Quantity<U> {
    fn format(&self, f: defmt::Formatter<'_>) {
        defmt_number(&self.value, f);
        let unit = U::EXPR;
        if let UnitExpr::Named { symbol, .. } = unit {
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
            UnitExpr::Named { symbol, .. } => defmt::write!(f, "{=str}", symbol),
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

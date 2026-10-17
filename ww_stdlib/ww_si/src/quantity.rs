//! Constructors, conversions, comparisons and formatting, written once for every [Quantity].

use crate::ops::{compare, scale_f32, scale_f64, to_i128_scaled};
use crate::unit::{Unit, UnitExpr};
use crate::{Prefix, Quantity};
use core::cmp::Ordering;
use core::fmt;
use core::marker::PhantomData;
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
        let x = scale_f32(x, -i8::from(prefix));
        let value = storage
            .from_f32_saturating(x)
            .expect("quantize: number type not enabled or not supported");
        Self::from_parts(prefix, value)
    }

    /// The value in the unit itself (no prefix) as `f64`, rounded where `f64` can't hold it exactly. For a unit with a
    /// scale (°, °C, g) this is still in that unit, [to_si_f64](Self::to_si_f64) converts to coherent SI.
    pub fn to_f64(&self) -> f64 {
        scale_f64(self.value.to_f64(), self.prefix.into())
    }

    /// The value in the unit itself (no prefix) as `f32`.
    pub fn to_f32(&self) -> f32 {
        scale_f32(self.value.as_f32(), self.prefix.into())
    }

    /// The value in units of `prefix` as `f64` (`to_f64_in(Prefix::Milli)` of 3.3 V is 3300.0).
    pub fn to_f64_in(&self, prefix: Prefix) -> f64 {
        scale_f64(
            self.value.to_f64(),
            i8::from(self.prefix) - i8::from(prefix),
        )
    }

    /// The value in units of `prefix` as an integer: exact for integer storage (truncated toward zero when `prefix` is
    /// bigger than the stored one), rounded for floats. `None` when it doesn't fit in `i128` or is `NaN`.
    pub fn to_i128_in(&self, prefix: Prefix) -> Option<i128> {
        to_i128_scaled(&self.value, i8::from(self.prefix) - i8::from(prefix))
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
        let from = i8::from(self.prefix);
        let to = i8::from(prefix);
        let value = match self.value {
            #[cfg(feature = "f32")]
            NumericValue::F32(x) => NumericValue::F32(scale_f32(x, from - to)),
            #[cfg(feature = "f64")]
            NumericValue::F64(x) => NumericValue::F64(scale_f64(x, from - to)),
            value => {
                let x = value.to_i128()?;
                let scaled = crate::ops::scale_i128(x, from - to)?;
                // truncated going to a bigger prefix: not exact
                if crate::ops::scale_i128(scaled, to - from) != Some(x) {
                    return None;
                }
                value.ty().from_i128(scaled)?
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
/// of magnitude apart), through `f64` otherwise.
impl<U: Unit> PartialOrd for Quantity<U> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        compare(
            self.prefix.into(),
            &self.value,
            other.prefix.into(),
            &other.value,
        )
    }
}

/// The number as stored: integers exactly, `f32` as `f32` (3.3, not 3.299999952316284).
pub(crate) fn fmt_number(value: &NumericValue, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match *value {
        #[cfg(feature = "f32")]
        NumericValue::F32(x) => write!(f, "{x}"),
        #[cfg(feature = "f64")]
        NumericValue::F64(x) => write!(f, "{x}"),
        value => match value.to_i128() {
            Some(x) => write!(f, "{x}"),
            // u128 above i128::MAX, UN / IN
            None => write!(f, "{}", value.to_f64()),
        },
    }
}

/// The prefix and the unit's symbol, as written after the number: `mV`, `kg`, `%` for a ratio in `Centi`.
fn fmt_unit(prefix: Prefix, unit: &UnitExpr, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let dimensionless = matches!(unit, UnitExpr::Named { symbol: "", .. });
    if dimensionless {
        return match prefix {
            Prefix::Unit => Ok(()),
            Prefix::Centi => f.write_str(" %"),
            Prefix::Milli => f.write_str(" ‰"),
            Prefix::Micro => f.write_str(" ppm"),
            p => write!(f, "e{}", i8::from(p)),
        };
    }
    let compound = !matches!(unit, UnitExpr::Named { .. });
    match (prefix, compound) {
        (Prefix::Unit, _) => write!(f, " {unit}"),
        (p, false) => write!(f, " {}{unit}", p.symbol()),
        (p, true) => write!(f, " {}({unit})", p.symbol()),
    }
}

/// `3300 mV`, the number as stored with its prefix; `{:#}` writes it in the unit itself: `3.3 V`.
impl<U: Unit> fmt::Display for Quantity<U> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            match self.value {
                #[cfg(feature = "f32")]
                NumericValue::F32(_) => write!(f, "{}", self.to_f32())?,
                _ => write!(f, "{}", self.to_f64())?,
            }
            fmt_unit(Prefix::Unit, U::EXPR, f)
        } else {
            fmt_number(&self.value, f)?;
            fmt_unit(self.prefix, U::EXPR, f)
        }
    }
}

/// `Volt(3300 mV U16)`: the unit's name, the value as `Display` shows it, and the number type.
impl<U: Unit> fmt::Debug for Quantity<U> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}(", U::EXPR.name())?;
        fmt_number(&self.value, f)?;
        fmt_unit(self.prefix, U::EXPR, f)?;
        write!(f, " {:?})", self.value.ty())
    }
}

/// `3300 mV`, like `Display`.
#[cfg(feature = "defmt")]
impl<U: Unit> defmt::Format for Quantity<U> {
    fn format(&self, f: defmt::Formatter<'_>) {
        match self.value {
            #[cfg(feature = "f32")]
            NumericValue::F32(x) => defmt::write!(f, "{=f32}", x),
            #[cfg(feature = "f64")]
            NumericValue::F64(x) => defmt::write!(f, "{=f64}", x),
            value => match value.to_i128() {
                Some(x) => defmt::write!(f, "{=i128}", x),
                None => defmt::write!(f, "?"),
            },
        }
        let unit = U::EXPR;
        if let UnitExpr::Named { symbol, .. } = unit {
            if symbol.is_empty() {
                match self.prefix {
                    Prefix::Unit => {}
                    Prefix::Centi => defmt::write!(f, " %"),
                    Prefix::Milli => defmt::write!(f, " ‰"),
                    Prefix::Micro => defmt::write!(f, " ppm"),
                    p => defmt::write!(f, "e{=i8}", i8::from(p)),
                }
            } else {
                defmt::write!(f, " {=str}{=str}", self.prefix.symbol(), symbol);
            }
        } else {
            // compound units are rare on a device: the type name instead of a rendered tree
            defmt::write!(f, " {=str}{}", self.prefix.symbol(), DefmtName(unit));
        }
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
        }
    }
}

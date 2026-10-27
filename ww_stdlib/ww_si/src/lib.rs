#![cfg_attr(not(feature = "std"), no_std)]
//! SI quantities for device APIs: a [Prefix] and a [NumericValue] in whatever number type the device picks, with the
//! unit in the type (`Volt::milli(3300u16)` is a [Quantity] of [units::Volt]), so the API says what a value means and
//! each device chooses its own resolution and range.
//!
//! Every quantity is the one generic [Quantity]`<U>`, and every unit a zero-size marker in [units]; the names APIs
//! write are aliases (`pub type Volt = Quantity<units::Volt>`). Constructors, conversions, comparisons and formatting
//! are written once for all of them, and `*` / `/` between quantities are checked by the compiler against a product
//! table (`Volt * Ampere` is a `Watt`).
//!
//! Quantity groups are cargo features (`time`, `electrical`, `mechanical`, `angle`, `thermal`, `chemical`,
//! `photometric`, `radiation`, `logarithmic`, `dimensionless`, `quantities` for all), as are the number types (`u16`,
//! `f32`, .., `all`, see `ww_numeric`) and the host helpers (`host`: [Unwrapper]). `std` enables everything.
//!
//! A value with the time it was taken is a [Timed] (feature `time`).
//!
//! A value whose unit, prefix and number type the API fixes (`Fixed<Second, Micro, u64>`, a timestamp in µs) is a
//! [Fixed]: on the wire exactly the number, the unit only in the type.
//!
//! A value whose unit is only known at run time (a configurable sensor channel) is an [AnyQuantity]: the same prefix
//! and number, and the unit as an [SIExpr], the tree a unit marker describes ([Unit::SI_EXPR]) put on the wire. It is
//! shown and converted like the typed quantity of the same unit.

mod algebra;
#[cfg(test)]
mod customary;
mod dynamic;
pub mod tree;
pub use dynamic::AnyQuantity;
#[cfg(feature = "std")]
pub use dynamic::AnyQuantityOwned;
mod exact;
pub use exact::Steps;
mod fixed;
mod ops;
mod quantity;
#[cfg(feature = "time")]
mod timed;
#[cfg(feature = "time")]
pub use timed::Timed;
#[cfg(feature = "host")]
mod timestamp;
#[cfg(feature = "host")]
pub use timestamp::{UnwrapError, Unwrapper};

pub use exact_units;
#[cfg(feature = "information")]
pub use exact_units::Bytes;
pub use exact_units::{
    Bounds, Counted, DecimalPrefix, Dim, Fixed, FixedScale, MAX_COUNTED, MAX_UNIT_DEPTH, Magnitude,
    Offset, Origin, OutOfBounds, Point, PointDiff, Prefix, Range, RangeDec, RangeU64, Repr, SIExpr,
    Scale, Storage, Unbounded, Unit, UnitDiv, UnitExpr, UnitMul, UnsignedStorage, counted, origin,
    origins, prefix, scale, unit, units,
};
#[cfg(feature = "std")]
pub use exact_units::{CountedOwned, DimOwned, SIExprOwned};
#[cfg(feature = "dimensionless")]
pub use exact_units::{Duty16, Ppb};
#[cfg(feature = "time")]
pub use exact_units::{Micros, Millis, Nanos};

use core::marker::PhantomData;
use shrink_wrap::prelude::*;
pub use ww_numeric;
use ww_numeric::NumericValue;

/// A value of the unit `U`: a prefix and a number in whatever storage the device picks. The unit is only in the type,
/// it takes no space on the wire or in RAM; `Quantity<units::Volt>` is written [Volt].
///
/// On the wire the prefix (a `UNib32`, one nibble up to `Ronna`) and then the value with its number type
/// (`final_structure`): `Volt::milli(3300u16)` is `12 e4 0c`. In a `Series` the prefix is a run and the value a
/// numeric column that restarts where the prefix changes.
///
/// ```
/// use ww_si::{Ampere, Volt, Watt};
///
/// let v = Volt::milli(250u8);
/// assert_eq!(v, Volt::base(0.25f32));
/// assert_eq!(format!("{v}"), "250 mV");
/// assert_eq!(format!("{v:#}"), "0.25 V");
/// let p: Watt = v * Ampere::base(2u8);
/// assert_eq!(p, Watt::base(0.5f32));
/// ```
///
/// Units that don't go together don't build. Only products in the table multiply:
///
/// ```compile_fail
/// use ww_si::{AngularVelocity, Volt};
///
/// let _ = Volt::base(1u8) * AngularVelocity::base(1u8);
/// ```
///
/// and only values of the same unit add up:
///
/// ```compile_fail
/// use ww_si::{Ampere, Volt};
///
/// let _ = Volt::base(1u8) + Ampere::base(1u8);
/// ```
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone),
    final_structure,
    series(cells)
)]
pub struct Quantity<U: Unit> {
    pub prefix: Prefix,
    pub value: NumericValue,
    _unit: PhantomData<U>,
}

/// Time in seconds with a prefix (s, ms, µs, ns), and a device timestamp, see below.
///
/// # As a timestamp
///
/// A `Second` in a device API that marks when something happened (a sample, an I2C transaction's start and end) is
/// the device's time since its clock started: one clock per device, so timestamps of all its traits line up. The
/// device picks prefix and storage (`U32` of µs, `U64` of ns, ..), they stay the same for a given stream or method.
///
/// Fixed-width unsigned storage holds the time modulo `2^bits`, it wraps: a `U32` of µs every 71.6 minutes, a `U16`
/// of ms every 65.5 s. `U64` and `UVlq64` don't wrap in practice (584 000 years of µs); `UVlq32` wraps like `U32`.
/// Timestamps are never signed or floats. The host gets the full time back with [Unwrapper] (feature `host`),
/// anchored on a full-width reading of the same clock (`ww_timebase::Clock::now()`).
#[cfg(feature = "time")]
pub type Second = Quantity<units::Second>;

/// Frequency in hertz with a prefix (Hz, kHz, MHz, GHz).
#[cfg(feature = "time")]
pub type Hertz = Quantity<units::Hertz>;

/// Frequency drift in hertz per second with a prefix (Hz/s, kHz/s).
#[cfg(feature = "time")]
pub type HertzPerSecond = Quantity<units::HertzPerSecond>;

/// Symbol rate in baud with a prefix (Bd, kBd, MBd): the baud rate of a UART, the bit rate of a decoder.
#[cfg(feature = "time")]
pub type Baud = Quantity<units::Baud>;

/// Electric current in amperes with a prefix (A, mA, µA, nA).
#[cfg(feature = "electrical")]
pub type Ampere = Quantity<units::Ampere>;

/// Voltage in volts with a prefix (V, mV, µV, kV).
#[cfg(feature = "electrical")]
pub type Volt = Quantity<units::Volt>;

/// Resistance in ohms with a prefix (Ω, kΩ, MΩ).
#[cfg(feature = "electrical")]
pub type Ohm = Quantity<units::Ohm>;

/// Capacitance in farads with a prefix (F, µF, nF, pF).
#[cfg(feature = "electrical")]
pub type Farad = Quantity<units::Farad>;

/// Conductance in siemens with a prefix (S, mS).
#[cfg(feature = "electrical")]
pub type Siemens = Quantity<units::Siemens>;

/// Inductance in henries with a prefix (H, mH, µH).
#[cfg(feature = "electrical")]
pub type Henry = Quantity<units::Henry>;

/// Electric charge in coulombs with a prefix (C, mC).
#[cfg(feature = "electrical")]
pub type Coulomb = Quantity<units::Coulomb>;

/// Power in watts with a prefix (W, mW, kW).
#[cfg(feature = "electrical")]
pub type Watt = Quantity<units::Watt>;

/// Magnetic flux in webers with a prefix (Wb, mWb).
#[cfg(feature = "electrical")]
pub type Weber = Quantity<units::Weber>;

/// Magnetic flux density in teslas with a prefix (T, mT).
#[cfg(feature = "electrical")]
pub type Tesla = Quantity<units::Tesla>;

/// Length in meters with a prefix (m, mm, µm, km).
#[cfg(feature = "mechanical")]
pub type Meter = Quantity<units::Meter>;

/// Mass in grams with a prefix (g, mg, kg): 5 kg is `Gram::kilo(5u8)`.
#[cfg(feature = "mechanical")]
pub type Gram = Quantity<units::Gram>;

/// Force in newtons with a prefix (N, mN, kN).
#[cfg(feature = "mechanical")]
pub type Newton = Quantity<units::Newton>;

/// Pressure in pascals with a prefix (Pa, kPa, MPa).
#[cfg(feature = "mechanical")]
pub type Pascal = Quantity<units::Pascal>;

/// Energy in joules with a prefix (J, mJ, kJ).
#[cfg(feature = "mechanical")]
pub type Joule = Quantity<units::Joule>;

/// Torque in newton-meters with a prefix (N·m, mN·m), kept apart from [Joule].
#[cfg(feature = "mechanical")]
pub type Torque = Quantity<units::Torque>;

/// Volume in litres with a prefix (L, mL, µL).
#[cfg(feature = "mechanical")]
pub type Litre = Quantity<units::Litre>;

/// Volumetric flow in cubic meters per second with a prefix.
#[cfg(feature = "mechanical")]
pub type VolumetricFlow = Quantity<units::VolumetricFlow>;

/// Speed in meters per second with a prefix (m/s, mm/s, km/s).
#[cfg(feature = "mechanical")]
pub type Speed = Quantity<units::Speed>;

/// Acceleration in meters per second squared with a prefix.
#[cfg(feature = "mechanical")]
pub type Acceleration = Quantity<units::Acceleration>;

/// Jerk in meters per second cubed with a prefix.
#[cfg(feature = "mechanical")]
pub type Jerk = Quantity<units::Jerk>;

/// Snap in meters per second to the fourth with a prefix.
#[cfg(feature = "mechanical")]
pub type Snap = Quantity<units::Snap>;

/// Yank in newtons per second with a prefix.
#[cfg(feature = "mechanical")]
pub type Yank = Quantity<units::Yank>;

/// Angle in radians with a prefix (rad, mrad).
#[cfg(feature = "angle")]
pub type Radian = Quantity<units::Radian>;

/// Angle in degrees with a prefix (°, m°).
#[cfg(feature = "angle")]
pub type Degree = Quantity<units::Degree>;

/// Angle in turns with a prefix (rev).
#[cfg(feature = "angle")]
pub type Turn = Quantity<units::Turn>;

/// Solid angle in steradians with a prefix.
#[cfg(feature = "angle")]
pub type Steradian = Quantity<units::Steradian>;

/// Angular velocity in radians per second with a prefix (rad/s, krad/s).
#[cfg(feature = "angle")]
pub type AngularVelocity = Quantity<units::AngularVelocity>;

/// Angular acceleration in radians per second squared with a prefix.
#[cfg(feature = "angle")]
pub type AngularAcceleration = Quantity<units::AngularAcceleration>;

/// Thermodynamic temperature in kelvins with a prefix (K, mK).
#[cfg(feature = "thermal")]
pub type Kelvin = Quantity<units::Kelvin>;

/// Temperature in degrees Celsius with a prefix (°C, m°C).
#[cfg(feature = "thermal")]
pub type DegreeCelsius = Quantity<units::DegreeCelsius>;

/// Amount of substance in moles with a prefix (mol, mmol, µmol).
#[cfg(feature = "chemical")]
pub type Mole = Quantity<units::Mole>;

/// Catalytic activity in katals with a prefix.
#[cfg(feature = "chemical")]
pub type Katal = Quantity<units::Katal>;

/// Luminous intensity in candelas with a prefix (cd, mcd).
#[cfg(feature = "photometric")]
pub type Candela = Quantity<units::Candela>;

/// Luminous flux in lumens with a prefix.
#[cfg(feature = "photometric")]
pub type Lumen = Quantity<units::Lumen>;

/// Illuminance in lux with a prefix.
#[cfg(feature = "photometric")]
pub type Lux = Quantity<units::Lux>;

/// Radioactivity in becquerels with a prefix.
#[cfg(feature = "radiation")]
pub type Becquerel = Quantity<units::Becquerel>;

/// Absorbed dose in grays with a prefix.
#[cfg(feature = "radiation")]
pub type Gray = Quantity<units::Gray>;

/// Equivalent dose in sieverts with a prefix.
#[cfg(feature = "radiation")]
pub type Sievert = Quantity<units::Sievert>;

/// Energy in electron-volts with a prefix (eV, keV, MeV).
#[cfg(feature = "radiation")]
pub type ElectronVolt = Quantity<units::ElectronVolt>;

/// A fraction with a prefix (1, %, ‰, ppm as `Unit`, `Centi`, `Milli`, `Micro`): a duty cycle, a probability, a
/// scale factor, an efficiency.
#[cfg(feature = "dimensionless")]
pub type Ratio = Quantity<units::Ratio>;

#[cfg(feature = "dimensionless")]
impl Ratio {
    /// `value` in percent, `Prefix::Centi`: `Ratio::percent(50u8)` is one half.
    pub fn percent(value: impl Into<NumericValue>) -> Self {
        Self::new(Prefix::Centi, value)
    }

    /// `value` in per mille, `Prefix::Milli`.
    pub fn per_mille(value: impl Into<NumericValue>) -> Self {
        Self::new(Prefix::Milli, value)
    }

    /// `value` in parts per million, `Prefix::Micro`.
    pub fn ppm(value: impl Into<NumericValue>) -> Self {
        Self::new(Prefix::Micro, value)
    }
}
/// A level in decibels, dB, without a prefix (a logarithm, not a [Ratio]).
#[cfg(feature = "logarithmic")]
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug),
    final_structure,
    series(cells)
)]
pub struct DeciBel {
    pub value: NumericValue,
}

/// A power level in decibels relative to 1 mW, dBm, without a prefix.
#[cfg(feature = "logarithmic")]
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug),
    final_structure,
    series(cells)
)]
pub struct DeciBelmW {
    pub value: NumericValue,
}

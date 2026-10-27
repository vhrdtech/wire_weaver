//! `ww_si` quantities in an API, as dynamic clients see them (CLT-31): a named unit, a `Series` of `Timed` values, a
//! compound unit (`Per`), a unit marker of this crate that `ww_si` doesn't know, and a value whose unit is only known
//! at run time (`AnyQuantity`, STD-37), and values with unit, prefix and storage fixed at compile time (`Fixed`,
//! STD-55), written with and without their default bounds.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::Series;
use ww_si::prefix::{Milli, One};
use ww_si::units::{AngularVelocity, Byte, Per, Second, Torque};
use ww_si::{
    Ampere, AnyQuantity, Dim, Fixed, Micros, Quantity, Range, Scale, Timed, Unbounded, Unit,
    UnitExpr, Volt,
};

#[ww_trait]
trait Quantities {
    property!(rw voltage: Volt);
    /// Current history with device time
    fn history() -> Series<'i, Timed<Ampere>>;
    /// Viscous damping: torque per angular velocity
    property!(rw damping: Quantity<Per<Torque, AngularVelocity>>);
    property!(rw speed: Quantity<Rpm>);
    /// A sensor input whose unit is set by configuration, sent with every value (STD-37)
    property!(rw sensor: AnyQuantity<'i>);
    /// `B` left out: the walker fills in `Unbounded`
    property!(rw timeout: Fixed<Second, Milli, u16>);
    /// The same type with `B` written out
    property!(rw timeout_full: Fixed<Second, Milli, u16, Unbounded>);
    property!(rw window: Fixed<Second, Milli, u16, Range<1, 5000>>);
    /// Sizes with device time, `B` left out inside `Timed` and a `Series`
    fn sizes() -> Series<'i, Timed<Fixed<Byte, One, u32>>>;
    /// Through an alias, `B` left out
    fn ticks() -> Series<'i, Timed<Micros<u32>>>;
}

/// Revolutions per minute
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rpm;

impl Unit for Rpm {
    // rad/s
    const DIM: Dim<'static> = Dim::new(-1, 0, 0, 0, 0, 0, 0).with_rad(1);
    const EXPR: &'static UnitExpr = &UnitExpr::Named {
        name: "Rpm",
        symbol: "rpm",
        dim: Self::DIM,
        scale: Scale::Factor(core::f64::consts::TAU / 60.0),
    };
}

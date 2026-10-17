//! `ww_si` quantities in an API, as dynamic clients see them (CLT-31): a named unit, a `Series` of `Timed` values, a
//! compound unit (`Per`), a unit marker of this crate that `ww_si` doesn't know, and a value whose unit is only known
//! at run time (`AnyQuantity`, STD-37).
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::Series;
use ww_si::units::{AngularVelocity, Per, Torque};
use ww_si::{Ampere, AnyQuantity, Dim, Quantity, Scale, Timed, Unit, UnitExpr, Volt};

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
}

/// Revolutions per minute
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rpm;

impl Unit for Rpm {
    const DIM: Dim = Dim::new(-1, 0, 0, 0, 0, 0, 0);
    const EXPR: &'static UnitExpr = &UnitExpr::Named {
        name: "Rpm",
        symbol: "rpm",
        dim: Self::DIM,
        scale: Scale::Factor(core::f64::consts::TAU / 60.0),
    };
}

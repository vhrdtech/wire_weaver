//! `ww_si` quantities in an API, as dynamic clients see them (CLT-31): a named unit, a `Series` of `Timed` values, a
//! compound unit (`Per`), a unit marker of this crate, declared with `unit!` so clients know it (INT-13), and a value whose unit is only known
//! at run time (`AnyQuantity`, STD-37), and values with unit, prefix and storage fixed at compile time (`Fixed`,
//! STD-55), written with and without their default bounds.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::Series;
use ww_si::prefix::{Milli, One};
use ww_si::units::{AngularVelocity, Byte, Per, Second, Torque};
use ww_si::{Ampere, AnyQuantity, Fixed, Micros, Quantity, Range, Timed, Unbounded, Volt};

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

ww_si::unit! {
    /// Revolutions per minute: π/30 rad/s
    pub struct Rpm = "rpm", [rad 1, s -1], Exact(1 / 30, pi 1);
}

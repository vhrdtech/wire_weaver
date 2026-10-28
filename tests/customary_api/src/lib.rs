//! The Mars Climate Orbiter case (STD-72): a ground software API reports thruster impulse in pound-force seconds, the
//! navigation software expects newton seconds. In 1999 the numbers were read as N·s, 4.45 times too small; here the
//! unit is part of every value's type, so a client converts exactly or refuses, never reinterprets.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
use ww_si::prefix::Milli;
use ww_si::units::{PoundForce, Second, Times};
use ww_si::{AnyQuantity, Fixed, Quantity};

/// Impulse in pound-force seconds, in steps of a thousandth.
pub type LbfSeconds = Fixed<Times<PoundForce, Second>, Milli, i32>;

#[ww_trait]
trait SmallForces {
    /// Impulse of the last angular momentum desaturation, as the ground software's table has it
    property!(rw impulse: LbfSeconds);
    /// The same impulse with the prefix and number type picked by the device
    property!(rw impulse_q: Quantity<Times<PoundForce, Second>>);
    /// The same impulse with its unit sent along
    property!(rw impulse_any: AnyQuantity<'i>);
}

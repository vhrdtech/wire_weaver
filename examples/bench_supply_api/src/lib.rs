//! API of the bench supply example device: a lab instrument with independent output channels, each with a
//! voltage and current limit, and an output switch that refuses instead of overloading the load (EX-19).
//!
//! Every number carries its unit, step and bounds in its type: a limit is `Fixed<Volt, Milli, u16, Range<0,
//! 30_000>>` or `Fixed<Ampere, Milli, u16, Range<0, 5_000>>`. On the wire each is exactly its integer.
#![cfg_attr(not(feature = "std"), no_std)]

use ww_api::prelude::*;
use ww_si::prefix::Milli;
use ww_si::units::{Ampere, Volt};
use ww_si::{Fixed, Range};

/// 0 to 30.000 V in steps of 1 mV.
pub type Voltage = Fixed<Volt, Milli, u16, Range<0, 30_000>>;
/// 0 to 5.000 A in steps of 1 mA.
pub type Current = Fixed<Ampere, Milli, u16, Range<0, 5_000>>;

// --8<-- [start:root]
/// The bench supply.
#[ww_api_root]
#[effect(edit)]
pub trait BenchSupply {
    /// The output channels, 0-based on the wire.
    ww_impl!(channel[]: Channel);
}
// --8<-- [end:root]

// --8<-- [start:channel]
/// One output channel.
#[ww_trait]
#[effect(edit)]
pub trait Channel {
    /// Highest voltage the output may be set to.
    property!(rw voltage_limit: Voltage);
    /// Highest current the output may draw; [set_output](Channel::set_output) refuses to turn the output on
    /// above it.
    property!(rw current_limit: Current);
    /// What the channel measures at its output terminals; zero while the output is off.
    property!(ro+observe measured: Measurement);
    /// Turns the output on or off; refuses to turn it on, see [OutputError].
    fn set_output(on: bool) -> Result<(), OutputError>;
    /// Whether the output is on.
    property!(ro+observe output_on: bool);
}
// --8<-- [end:channel]

/// One measurement at a channel's output terminals.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Clone, Copy)
)]
pub struct Measurement {
    pub voltage: Voltage,
    pub current: Current,
}

// --8<-- [start:error]
/// Why [Channel::set_output] refused to turn the output on.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Clone, Copy),
    ww_repr = u4,
    display
)]
pub enum OutputError {
    /// `voltage_limit` is still 0 V: nothing to turn on.
    NoVoltageSet,
    /// At `voltage_limit` the load would draw {would_draw}, above `current_limit`.
    OverCurrent { would_draw: Current },
}
// --8<-- [end:error]

ww_api::api_error!(OutputError);

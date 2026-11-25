//! API of the thermometer example device: a sensor board with up to four temperature inputs. It reads a
//! temperature once, samples continuously at a rate the host sets, streams the readings with their sequence
//! numbers, takes commands, and reports an amplifier gain that the device may change by itself.
//!
//! Every number carries its unit, step and bounds in its type: the rate is `Fixed<Hertz, One, u16, Range<1, 1000>>`,
//! a temperature `Point<Celsius, Fixed<Kelvin, Centi, i16>>`. On the wire each is exactly its integer.
//!
//! This is the first release, wire line 1 at w1.0. It is the same wire as line 1 of `tests/thermo_api`, the
//! multi-line bench that tells this device's later life.
#![cfg_attr(not(feature = "std"), no_std)]

use ww_api::prelude::*;
use ww_si::origins::Celsius;
use ww_si::prefix::{Centi, One};
use ww_si::units::{Hertz, Item, Kelvin, Ratio, Sample as SampleUnit};
use ww_si::{Fixed, IndexFrom1, Point, Range, Seq};

/// Sampling rate: 1 Hz to 1000 Hz in steps of 1 Hz.
pub type Rate = Fixed<Hertz, One, u16, Range<1, 1000>>;

// --8<-- [start:gain]
/// Amplifier gain: 1 to 16, no unit.
pub type Gain = Fixed<Ratio, One, u8, Range<1, 16>>;
// --8<-- [end:gain]

/// A temperature in steps of 0.01 K, from -327.68 °C to 327.67 °C.
pub type Temperature = Point<Celsius, Fixed<Kelvin, Centi, i16>>;

// --8<-- [start:root]
/// The thermometer.
#[ww_api_root]
#[effect(edit)]
pub trait Thermo {
    /// Sets how often the device samples while `mode` is `Continuous`. `Busy` while a triggered sample runs.
    fn set_rate(rate: Fixed<Hertz, One, u16, Range<1, 1000>>) -> Result<(), RateError>;
    /// Takes one sample on the first input now, whatever the mode.
    fn read_once() -> Sample;
    /// What the sampler does.
    property!(rw mode: Mode);
    /// Amplifier gain. Observable: the device lowers it by itself when the input overloads.
    property!(rw+observe gain: Fixed<Ratio, One, u8, Range<1, 16>>);
    /// Samples while `mode` is `Continuous`, one per `Command::Trigger` while `Triggered`.
    stream!(samples: Sample);
    /// Commands for the sampler, in order.
    sink!(commands: Command);
    /// The inputs, 0-based on the wire. The device says how many it has.
    ww_impl!(channel[]: Channel);
    /// The status LED.
    ww_impl!(indication: Indication);
}
// --8<-- [end:root]

/// One temperature input.
#[ww_trait]
#[effect(edit)]
pub trait Channel {
    /// Whether the sampler reads this input.
    property!(rw enabled: bool);
    /// The last sample taken on this input.
    fn last() -> Sample;
}

// --8<-- [start:sample]
/// One temperature reading.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone, Copy)
)]
pub struct Sample {
    /// Counts up by one per sample and wraps; `Command::Mark` sets it.
    pub seq: Seq<SampleUnit, u16>,
    /// The input it was taken on, 1 to 4 as printed on the board.
    pub input: IndexFrom1<Item, u8, Range<1, 4>>,
    /// In steps of 0.01 K, from -327.68 °C to 327.67 °C.
    pub temp: Point<Celsius, Fixed<Kelvin, Centi, i16>>,
}
// --8<-- [end:sample]

// --8<-- [start:mode]
/// What the sampler does.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy), ww_repr = u4)]
pub enum Mode {
    /// Takes no samples on its own.
    Idle,
    /// Samples at the rate `set_rate` set.
    Continuous,
    /// One sample per `Command::Trigger`.
    Triggered,
}
// --8<-- [end:mode]

/// Why `set_rate` refused.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone, Copy),
    ww_repr = u4,
    display
)]
pub enum RateError {
    /// A triggered sample is running.
    Busy,
}

ww_api::api_error!(RateError);

/// A command for the sampler.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy), ww_repr = u4)]
pub enum Command {
    /// Takes one sample now; only in `Mode::Triggered`.
    Trigger,
    /// Restarts sample numbering at `seq`.
    Mark { seq: Seq<SampleUnit, u16> },
}

/// The status LED.
#[ww_trait]
#[effect(edit)]
pub trait Indication {
    /// How much the LED shows.
    property!(rw mode: IndicationMode);
}

/// How much the status LED shows, see [Indication::mode].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq),
    ww_repr = nib
)]
pub enum IndicationMode {
    /// Always off.
    AllOff,
    /// Only errors.
    OnlyEssential,
    /// Errors and activity.
    Normal,
}

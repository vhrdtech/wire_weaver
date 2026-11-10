//! `thermo_api` 1.0.0 at wire w1.0: the first revision of line 1 (today `tests/thermo_api`'s `wire_v1`), before
//! `calibrate` (w1.1) and `uptime` (w1.2). Firmware in the field may still run it; the bench's host has no client of
//! it, it talks to it with w1.2's.
//!
//! Line 1 of the dual-driver bench (EVO-28), a small temperature logger.
//!
//! It touches every place a wire break can bite: methods with arguments, returns and errors, properties with get,
//! set and observe, an enum, a stream up and a sink down, an array of sub-levels, items added within the line
//! (`#[since]`), unit types from `ww_si` and a trait from `ww_stdlib` (`ww_indication`), both shared with line 2.
//! Line 2 (`wire_v2`) breaks the wire; `tests/thermo` bundles a client of each line.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
use ww_si::origins::Celsius;
use ww_si::prefix::{Centi, One};
use ww_si::units::{Hertz, Item, Kelvin, Ratio, Sample as SampleUnit};
use ww_si::{Fixed, IndexFrom1, Point, Range, Seq};

#[ww_api_root(wire = "w1.0")]
pub trait Thermo {
    /// How often the device samples while `mode` is `Continuous`. `Busy` while a triggered sample is running.
    fn set_rate(rate: Fixed<Hertz, One, u16, Range<1, 1000>>) -> Result<(), RateError>;
    /// Takes one sample now, whatever the mode.
    fn read_once() -> Sample;
    property!(rw mode: Mode);
    /// Amplifier gain, observable: the device changes it itself on overload.
    property!(rw+observe gain: Fixed<Ratio, One, u8, Range<1, 16>>);
    /// Samples while `mode` is `Continuous`, one per trigger while `Triggered`.
    stream!(samples: Sample);
    /// Commands for the sampler, in order.
    sink!(commands: Command);
    /// Input channels, 0-based on the wire.
    ww_impl!(channel[]: Channel);
    /// The status LED, a `ww_stdlib` trait both lines share.
    ww_impl!(indication: ww_indication::Indication);
}

#[ww_trait]
pub trait Channel {
    property!(rw enabled: bool);
    /// The last sample taken on this channel.
    fn last() -> Sample;
}

/// One temperature reading.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone, Copy)
)]
pub struct Sample {
    pub seq: Seq<SampleUnit, u16>,
    /// The input it was taken on, 1 to 4 as printed on the board.
    pub input: IndexFrom1<Item, u8, Range<1, 4>>,
    /// In steps of 0.01 K, from -327.68 °C to 327.67 °C.
    pub temp: Point<Celsius, Fixed<Kelvin, Centi, i16>>,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy), ww_repr = u4)]
pub enum Mode {
    Idle,
    Continuous,
    /// One sample per `Command::Trigger`.
    Triggered,
}

/// Unchanged in line 2, yet a type of its own there.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy), ww_repr = u4)]
pub enum RateError {
    Busy,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy), ww_repr = u4)]
pub enum Command {
    /// Take one sample now. Only in `Mode::Triggered`.
    Trigger,
    /// Restart sample numbering at `seq`.
    Mark { seq: Seq<SampleUnit, u16> },
}

//! `thermo_api` 2.0.0 at wire w2.0: the first revision of line 2 (`../w2`), before `self_test` (w2.1). The bench's
//! host has no client of it, it talks to it with w2.1's.
//!
//! Line 2 of the dual-driver bench (EVO-28), after a wire break of line 1.
//!
//! What broke against w1.2 (`../w1`):
//! - `set_rate(rate)` is gone, `configure(Config)` sets rate and mode in one call.
//! - `Sample::temp` is in 0.001 K steps in an `i32`, was 0.01 K in an `i16`.
//! - `Mode::Triggered` is gone: `Command::Trigger` takes a sample in any mode.
//! - `gain` is in steps of 0.01, was whole steps.
//! - `calibrate` takes 0.001 K steps.
//!
//! Unchanged: `read_once`, `mode`, the stream and sink, the channels, `uptime`, `RateError`, `Command` and the shared
//! `ww_indication` trait.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
use ww_si::origins::Celsius;
use ww_si::prefix::{Centi, Milli, One};
use ww_si::units::{Hertz, Item, Kelvin, Ratio, Sample as SampleUnit, Second};
use ww_si::{Fixed, IndexFrom1, Point, Range, Seq};

#[ww_api_root(wire = "w2.0")]
pub trait Thermo {
    /// Sets the sampling rate and mode together. `Busy` while a triggered sample is running.
    fn configure(config: Config) -> Result<(), RateError>;
    /// Takes one sample now, whatever the mode.
    fn read_once() -> Sample;
    property!(rw mode: Mode);
    /// Amplifier gain, observable: the device changes it itself on overload.
    property!(rw+observe gain: Fixed<Ratio, Centi, u16, Range<100, 1600>>);
    /// Samples while `mode` is `Continuous`, and one per `Command::Trigger`.
    stream!(samples: Sample);
    /// Commands for the sampler, in order.
    sink!(commands: Command);
    /// Input channels, 0-based on the wire.
    ww_impl!(channel[]: Channel);
    /// The status LED, a `ww_stdlib` trait both lines share.
    ww_impl!(indication: ww_indication::Indication);
    /// Adds `offset` to every sample from now on.
    fn calibrate(offset: Fixed<Kelvin, Milli, i32>);
    /// Time since boot.
    property!(ro uptime: Fixed<Second, One, u32>);
}

#[ww_trait]
pub trait Channel {
    property!(rw enabled: bool);
    /// The last sample taken on this channel.
    fn last() -> Sample;
}

/// One temperature reading.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy))]
pub struct Sample {
    pub seq: Seq<SampleUnit, u16>,
    /// The input it was taken on, 1 to 4 as printed on the board.
    pub input: IndexFrom1<Item, u8, Range<1, 4>>,
    /// In steps of 0.001 K.
    pub temp: Point<Celsius, Fixed<Kelvin, Milli, i32>>,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy))]
pub struct Config {
    pub rate: Fixed<Hertz, One, u16, Range<1, 1000>>,
    pub mode: Mode,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy), ww_repr = u4)]
pub enum Mode {
    Idle,
    Continuous,
}

/// Unchanged from line 1, yet a type of its own here.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy), ww_repr = u4)]
pub enum RateError {
    Busy,
}

/// Unchanged from line 1, yet a type of its own here.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy), ww_repr = u4)]
pub enum Command {
    /// Take one sample now.
    Trigger,
    /// Restart sample numbering at `seq`.
    Mark { seq: Seq<SampleUnit, u16> },
}

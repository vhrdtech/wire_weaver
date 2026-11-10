//! `thermo_api` 3.0.0 at wire w3.0: line 3 of the dual-driver bench (EVO-28), after a second wire break.
//!
//! What broke against w2.1 (`../w2`), each of a kind the w1 -> w2 break didn't have:
//! - The stream sends batches: `samples` carries a `Batch` of temperatures of one input with the first one's number,
//!   no longer one `Sample` per item. A host wanting single samples splits each batch.
//! - The tree moved: `uptime` and `self_test` are in a new sub-level, `health`, so their paths changed. `self_test`
//!   is the same resource, marked `#[moved_from]`; `uptime` changed too, so it is a new one.
//! - `uptime` is in milliseconds in a `u64`, was whole seconds in a `u32`: a host model in seconds can't hold it.
//!
//! Unchanged from w2.1: `configure`, `read_once`, `mode`, `gain`, the sink, the channels, `calibrate`, `Sample`,
//! `Config`, `Mode`, `RateError`, `Command`, `SelfTestError` and the shared `ww_indication` trait.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
use ww_si::origins::Celsius;
use ww_si::prefix::{Centi, Milli, One};
use ww_si::units::{Hertz, Item, Kelvin, Ratio, Sample as SampleUnit, Second};
use ww_si::{Fixed, IndexFrom1, Point, Range, Seq};

#[ww_api_root(wire = "w3.0")]
pub trait Thermo {
    /// Sets the sampling rate and mode together. `Busy` while a triggered sample is running.
    fn configure(config: Config) -> Result<(), RateError>;
    /// Takes one sample now, whatever the mode.
    fn read_once() -> Sample;
    property!(rw mode: Mode);
    /// Amplifier gain, observable: the device changes it itself on overload.
    property!(rw+observe gain: Fixed<Ratio, Centi, u16, Range<100, 1600>>);
    /// Samples in batches while `mode` is `Continuous`, and a burst of consecutive samples per `Command::Trigger`.
    stream!(samples: Batch<'i>);
    /// Commands for the sampler, in order.
    sink!(commands: Command);
    /// Input channels, 0-based on the wire.
    ww_impl!(channel[]: Channel);
    /// The status LED, a `ww_stdlib` trait every line shares.
    ww_impl!(indication: ww_indication::Indication);
    /// Adds `offset` to every sample from now on.
    fn calibrate(offset: Fixed<Kelvin, Milli, i32>);
    /// Uptime and the sensor check, moved here from the root.
    ww_impl!(health: Health);
}

#[ww_trait]
pub trait Channel {
    property!(rw enabled: bool);
    /// The last sample taken on this channel.
    fn last() -> Sample;
}

#[ww_trait]
pub trait Health {
    /// Time since boot.
    property!(ro uptime: Fixed<Second, Milli, u64>);
    /// Checks the sensor.
    #[moved_from(w2.1, Thermo::self_test)]
    fn self_test() -> Result<(), SelfTestError>;
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

/// Consecutive readings of one input: `temps[i]` is sample number `first + i`.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone))]
pub struct Batch<'i> {
    pub first: Seq<SampleUnit, u16>,
    pub input: IndexFrom1<Item, u8, Range<1, 4>>,
    pub temps: RefVec<'i, Point<Celsius, Fixed<Kelvin, Milli, i32>>>,
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

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy), ww_repr = u4)]
pub enum RateError {
    Busy,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy), ww_repr = u4)]
pub enum Command {
    /// Take one sample now.
    Trigger,
    /// Restart sample numbering at `seq`.
    Mark { seq: Seq<SampleUnit, u16> },
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy), ww_repr = u4)]
pub enum SelfTestError {
    SensorOpen,
    SensorShorted,
}

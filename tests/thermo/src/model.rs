//! The driver's own types, the same whatever line the device is on: shaped like the newest line, with what an older
//! line has and the newest dropped kept and marked (`Mode::Triggered`).
//!
//! Fields of `ww_si` types (`Seq`, `IndexFrom1`, `Fixed`, `Point`) are the very same Rust types in both API lines, as
//! both take `ww_si` from one crate: only the API crate's own structs and enums need converting.

use ww_si::origins::Celsius;
use ww_si::prefix::{Centi, Milli, One};
use ww_si::units::{Hertz, Item, Kelvin, Ratio, Sample as SampleUnit, Second};
use ww_si::{Fixed, IndexFrom1, Point, Range, Seq};

/// A temperature in 0.001 K steps: line 2's resolution, line 1's 0.01 K converts exactly.
pub type Temp = Point<Celsius, Fixed<Kelvin, Milli, i32>>;
/// A temperature offset, see [Temp].
pub type Offset = Fixed<Kelvin, Milli, i32>;
/// Gain in steps of 0.01 (line 2); line 1 takes whole steps only.
pub type Gain = Fixed<Ratio, Centi, u16, Range<100, 1600>>;
/// Sampling rate, the same type in both lines.
pub type Rate = Fixed<Hertz, One, u16, Range<1, 1000>>;
/// Time since boot, the same type in both lines.
pub type Uptime = Fixed<Second, One, u32>;
/// Sample number, the same type in both lines.
pub type SampleSeq = Seq<SampleUnit, u16>;
/// Input a sample was taken on, the same type in both lines.
pub type Input = IndexFrom1<Item, u8, Range<1, 4>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub seq: SampleSeq,
    pub input: Input,
    pub temp: Temp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Idle,
    Continuous,
    /// One sample per [Command::Trigger]. Line 1 only: line 2 takes a triggered sample in any mode.
    Triggered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Take one sample now (on line 1 only in [Mode::Triggered]).
    Trigger,
    /// Restart sample numbering at `seq`.
    Mark { seq: SampleSeq },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfTestError {
    SensorOpen,
    SensorShorted,
}

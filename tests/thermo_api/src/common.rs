//! What more than one line uses unchanged: one Rust type each, so a host needs no conversion between the lines for
//! it. A change here changes every line that uses it, and the checker names each (EVO-39).
//!
//! [RateError] and [Command] are on every line; the rest came with line 2's break and line 3 kept them.

use wire_weaver::prelude::*;
use ww_si::origins::Celsius;
use ww_si::prefix::{Milli, One};
use ww_si::units::{Hertz, Item, Kelvin, Sample as SampleUnit};
use ww_si::{Fixed, IndexFrom1, Point, Range, Seq};

/// Lines 2 and 3.
#[ww_trait]
pub trait Channel {
    property!(rw enabled: bool);
    /// The last sample taken on this channel.
    fn last() -> Sample;
}

/// One temperature reading. Lines 2 and 3.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone, Copy)
)]
pub struct Sample {
    pub seq: Seq<SampleUnit, u16>,
    /// The input it was taken on, 1 to 4 as printed on the board.
    pub input: IndexFrom1<Item, u8, Range<1, 4>>,
    /// In steps of 0.001 K.
    pub temp: Point<Celsius, Fixed<Kelvin, Milli, i32>>,
}

/// Lines 2 and 3.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone, Copy)
)]
pub struct Config {
    pub rate: Fixed<Hertz, One, u16, Range<1, 1000>>,
    pub mode: Mode,
}

/// Lines 2 and 3; line 1 has a `Triggered` mode of its own ([super::wire_v1::Mode]).
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy), ww_repr = u4)]
pub enum Mode {
    Idle,
    Continuous,
}

/// Every line.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy), ww_repr = u4)]
pub enum RateError {
    Busy,
}

/// Every line.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy), ww_repr = u4)]
pub enum Command {
    /// Take one sample now; on line 1 only in its `Mode::Triggered`.
    Trigger,
    /// Restart sample numbering at `seq`.
    Mark { seq: Seq<SampleUnit, u16> },
}

/// Lines 2 and 3.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy), ww_repr = u4)]
pub enum SelfTestError {
    SensorOpen,
    SensorShorted,
}

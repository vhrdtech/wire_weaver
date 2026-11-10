//! Line 1 at w1.2, the crate 1.2.0 before. `calibrate` came in w1.1, `uptime` in w1.2.

use super::common::{Command, RateError};
use wire_weaver::prelude::*;
use ww_si::origins::Celsius;
use ww_si::prefix::{Centi, One};
use ww_si::units::{Hertz, Item, Kelvin, Ratio, Sample as SampleUnit, Second};
use ww_si::{Fixed, IndexFrom1, Point, Range, Seq};

#[ww_api_root]
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
    /// The status LED, a `ww_stdlib` trait every line shares.
    ww_impl!(indication: ww_indication::Indication);
    /// Adds `offset` to every sample from now on. Added in w1.1.
    #[since = "w1.1"]
    fn calibrate(offset: Fixed<Kelvin, Centi, i16>);
    /// Time since boot. Added in w1.2.
    #[since = "w1.2"]
    property!(ro uptime: Fixed<Second, One, u32>);
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

//! Line 2 at w6.1, at w6.0 in 5.0.0 (the crate 2.1.0 had it at w2.1, before R7's indication swap moved it to a new
//! line). What broke against line 1:
//! - `set_rate(rate)` is gone, `configure(Config)` sets rate and mode in one call.
//! - `Sample::temp` is in 0.001 K steps in an `i32`, was 0.01 K in an `i16`.
//! - `Mode::Triggered` is gone: `Command::Trigger` takes a sample in any mode.
//! - `gain` is in steps of 0.01, was whole steps.
//! - `calibrate` takes 0.001 K steps.
//!
//! `self_test` came in w6.1, so a w6.0 device doesn't have it.

use super::common::{Channel, Command, Config, Indication, Mode, RateError, Sample, SelfTestError};
use ww_api::prelude::*;
use ww_si::prefix::{Centi, Milli, One};
use ww_si::units::{Kelvin, Ratio, Second};
use ww_si::{Fixed, Range};

#[ww_api_root]
#[effect(edit)]
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
    /// The status LED, a local stand-in for a `ww_stdlib` trait every line shares (R7).
    ww_impl!(indication: Indication);
    /// Adds `offset` to every sample from now on.
    fn calibrate(offset: Fixed<Kelvin, Milli, i32>);
    /// Time since boot.
    property!(ro uptime: Fixed<Second, One, u32>);
    /// Checks the sensor. Added in w6.1.
    #[since = "w6.1"]
    fn self_test() -> Result<(), SelfTestError>;
}

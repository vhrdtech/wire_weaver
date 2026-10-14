//! PWM outputs and pulse measurement over WireWeaver.
//!
//! | Trait        | What it is for                                                       | Who implements it          |
//! |--------------|----------------------------------------------------------------------|----------------------------|
//! | `Pwm`        | `channel[]` of PWM outputs: frequency, duty, polarity.               | Every device with PWM      |
//! | `PwmCapture` | `channel[]` of inputs measuring frequency, duty, pulse width, edges. | Devices with timer capture |
//!
//! Channels in one group share a timer and so a frequency ([PwmChannelInfo::group]).
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_si::{Hertz, Second};

/// Most channels a device lists.
pub const MAX_CHANNELS: usize = 32;

#[ww_trait]
pub trait Pwm {
    ww_impl!(channel[]: PwmChannel);

    fn capabilities() -> PwmCapabilities<'i>;
}

#[ww_trait]
pub trait PwmChannel {
    /// Starts or changes the output and returns what was actually set.
    fn set_output(output: PwmOutput) -> Result<PwmActual, PwmError>;

    /// Changes only the duty (0..=65535 = 0..100 %), keeping the frequency; returns the duty actually set.
    fn set_duty(duty: u16) -> Result<u16, PwmError>;

    /// Stops the output, leaving the pin high or low.
    fn stop(idle_high: bool) -> Result<(), PwmError>;

    fn state() -> PwmState;
}

#[ww_trait]
pub trait PwmCapture {
    ww_impl!(channel[]: PwmCaptureChannel);
}

#[ww_trait]
pub trait PwmCaptureChannel {
    /// Measurements, one per `gate`, while capture is on.
    stream!(capture: CaptureSample);

    fn set_capture(config: CaptureConfig) -> Result<CaptureConfig, PwmError>;

    /// The last measurement.
    fn read_capture() -> Result<CaptureSample, PwmError>;
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug))]
pub struct PwmOutput {
    pub freq: Hertz,
    /// 0..=65535 = 0..100 %.
    pub duty: u16,
    pub inverted: bool,
    pub phase_correct: bool,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug))]
pub struct PwmActual {
    pub freq: Hertz,
    pub duty: u16,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug), ww_repr = unib32)]
pub enum PwmState {
    Idle,
    Output(PwmActual),
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug))]
pub struct CaptureConfig {
    pub mode: CaptureMode,
    /// Measurement window.
    pub gate: Second,
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum CaptureMode {
    Off,
    Frequency,
    DutyCycle,
    PulseWidth,
    EdgeCount,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug))]
pub struct CaptureSample {
    /// End of the measurement window.
    pub t: Second,
    pub value: CaptureValue,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug), ww_repr = unib32)]
pub enum CaptureValue {
    Frequency(Hertz),
    /// 0..=65535 = 0..100 %.
    DutyCycle(u16),
    PulseWidth(Second),
    EdgeCount(u32),
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug))]
pub struct PwmCapabilities<'i> {
    pub channels: MaxLen<RefVec<'i, PwmChannelInfo<'i>>, MAX_CHANNELS>,
    pub min_freq: Hertz,
    pub max_freq: Hertz,
    /// Duty resolution of the hardware.
    pub duty_bits: u8,
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug))]
pub struct PwmChannelInfo<'i> {
    pub name: MaxLen<&'i str, 16>,
    /// Channels with the same group share the frequency.
    pub group: u8,
    /// This channel is also in `PwmCapture`.
    pub capture: bool,
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = unib32
)]
pub enum PwmError {
    Unsupported,
    InUse,
    NoIoLevel,
    Refused,
    InvalidArgument,
    WrongMode,
    /// The other channel of the group runs at another frequency.
    SharedTimer,
}

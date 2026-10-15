//! PWM outputs and pulse measurement over WireWeaver.
//!
//! # Traits
//!
//! | Trait        | What it is for                                                       | Who implements it          |
//! |--------------|------------------------------------------------------------------------|----------------------------|
//! | `Pwm`        | `channel[]` of PWM outputs: frequency, duty, polarity.               | Every device with PWM      |
//! | `PwmCapture` | `channel[]` of inputs measuring frequency, duty, pulse width, edges. | Devices with timer capture |
//!
//! ```rust,ignore
//! #[ww_api_root]
//! trait MyController {
//!     ww_impl!(pwm: ww_pwm::Pwm);
//!     ww_impl!(pwm_capture: ww_pwm::PwmCapture);
//! }
//! ```
//!
//! # Channels and groups
//!
//! Channel `n` of every method is `channels[n]` of [PwmCapabilities::channels], the order the device lists them.
//! Channels that share a timer share a frequency too ([PwmChannelInfo::group]): `PwmChannel::set_output` on one
//! channel changes it for the whole group; asking another channel of the same group for a different frequency
//! fails with [PwmError::SharedTimer]. [PwmChannelInfo::capture] says whether a channel is also reachable through
//! `PwmCapture`, at the same index.
//!
//! `PwmCapture` is its own trait (not a mode of `PwmChannel`): measuring pulses needs a timer in capture/input
//! mode, which most devices with PWM output don't have spare on every channel, so a plain PWM driver implements
//! `Pwm` only.
//!
//! # Streams
//!
//! `PwmCaptureChannel::capture` is a counted stream function: the host opens it with what to measure and over which
//! window, and gets one [CaptureValue] per `gate` until it closes the stream. The event head carries the time of
//! each measurement (the end of its window) and how many measurements the device dropped. Without a stream,
//! `PwmCaptureChannel::set_capture` and `PwmCaptureChannel::read_capture` measure and poll the last result.
//!
//! # Errors
//!
//! All traits return [PwmError]: the framework's `CommonError` as [PwmError::Common], plus `SharedTimer`. Each
//! method lists the variants it returns (`#[errors]`).
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_si::{Hertz, Prefix, Second};

/// Most channels a device lists, in [PwmCapabilities::channels] and `PwmCapture`'s own `channel[]`.
pub const MAX_CHANNELS: usize = 32;

/// Longest channel name ([PwmChannelInfo::name]).
pub const MAX_NAME_LEN: usize = 16;

/// PWM outputs: frequency, duty cycle and polarity, channel by channel.
#[ww_trait]
pub trait Pwm {
    /// One PWM output per channel; channel `n` is `channels[n]` of `Pwm::capabilities`.
    ww_impl!(channel[]: PwmChannel);

    /// The channels, their groups and the frequency and duty range.
    fn capabilities() -> PwmCapabilities<'i>;
}

/// One PWM output, see [PwmCapabilities::channels] for its group and whether it also captures.
#[ww_trait]
pub trait PwmChannel {
    /// Starts or changes the output and returns what was actually set (the nearest frequency and duty the hardware
    /// can do).
    /// `InvalidArgument`: frequency or duty out of range; `SharedTimer`: another channel of the same group already runs
    /// at a different frequency.
    #[errors(Common(InvalidArgument, InUse, NoPower, Refused), SharedTimer)]
    fn set_output(output: PwmOutput) -> Result<PwmActual, PwmError<'i>>;

    /// Changes only the duty (0..=65535 = 0..100 %), keeping the frequency; returns the duty actually set.
    /// `WrongState`: the channel is idle, `PwmChannel::set_output` first.
    #[errors(Common(WrongState, InvalidArgument))]
    fn set_duty(duty: u16) -> Result<u16, PwmError<'i>>;

    /// Stops the output, leaving the pin high or low.
    #[errors(Common(InUse, Refused))]
    fn stop(idle_high: bool) -> Result<(), PwmError<'i>>;

    /// The current output, or [PwmState::Idle] if stopped.
    fn state() -> PwmState;
}

/// Pulse measurement: frequency, duty cycle, pulse width or edge count, channel by channel.
#[ww_trait]
pub trait PwmCapture {
    /// One capture input per channel; channel `n` is the same index as `Pwm`'s, see [PwmChannelInfo::capture].
    ww_impl!(channel[]: PwmCaptureChannel);
}

/// One capture input, measuring over successive `gate` windows: streamed, or polled.
#[ww_trait]
pub trait PwmCaptureChannel {
    /// Measurements of `config`, one per `gate`, until the host closes the stream. Opening returns what was actually
    /// set (the nearest gate the hardware can do). Unit: measurement; the head's time is the end of its window.
    ///
    /// # Errors
    ///
    /// `Common(Unsupported)` (this mode isn't available on this channel), `Common(InvalidArgument)` (gate out of range,
    /// or [CaptureMode::Off]), `Common(InUse)`, `Common(Refused)`.
    #[stream(CaptureValue, counted)]
    fn capture(config: CaptureConfig) -> Result<CaptureConfig, PwmError<'i>>;

    /// Sets up what to measure without a stream, for `PwmCaptureChannel::read_capture`, and returns what was
    /// actually set (the nearest gate the hardware can do).
    /// `Unsupported`: this mode isn't available on this channel; `InvalidArgument`: gate out of range.
    #[errors(Common(Unsupported, InvalidArgument, InUse, Refused))]
    fn set_capture(config: CaptureConfig) -> Result<CaptureConfig, PwmError<'i>>;

    /// The last measurement.
    /// `WrongState`: no measurement yet; `PwmCaptureChannel::set_capture` first, then wait for at least one `gate`.
    #[errors(Common(WrongState))]
    fn read_capture() -> Result<CaptureSample, PwmError<'i>>;
}

/// What to set, see `PwmChannel::set_output`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug)
)]
pub struct PwmOutput {
    pub freq: Hertz,
    /// 0..=65535 = 0..100 %.
    pub duty: u16,
    /// Active level inverted: idle low instead of high, or the reverse.
    pub inverted: bool,
    /// Symmetric (center-aligned) counting instead of edge-aligned; usually halves the usable frequency range for
    /// the same duty resolution.
    pub phase_correct: bool,
}

/// What `PwmChannel::set_output` or `PwmChannel::set_duty` actually set, see [PwmState::Output].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug)
)]
pub struct PwmActual {
    pub freq: Hertz,
    /// 0..=65535 = 0..100 %.
    pub duty: u16,
}

/// The channel's output, see `PwmChannel::state`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug),
    ww_repr = unib32
)]
pub enum PwmState {
    /// Stopped: `PwmChannel::stop` was called, or the channel never started.
    Idle,
    /// Running, with what was actually set.
    Output(PwmActual),
}

/// What to measure, see `PwmCaptureChannel::capture` and `PwmCaptureChannel::set_capture`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug)
)]
pub struct CaptureConfig {
    pub mode: CaptureMode,
    /// Measurement window: for [CaptureMode::Frequency], [CaptureMode::DutyCycle] and [CaptureMode::EdgeCount], how
    /// long one measurement covers; [CaptureMode::PulseWidth] times the next single pulse, so `gate` only bounds
    /// how long it waits for one to start.
    pub gate: Second,
}

/// What `PwmCaptureChannel::set_capture` measures, see [CaptureValue] for the matching result.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum CaptureMode {
    /// No measurement; the channel is free for another function.
    Off,
    /// Rising-edge to rising-edge, averaged over `gate`.
    Frequency,
    /// High time over the period, averaged over `gate`.
    DutyCycle,
    /// A single pulse's high (or low, device-defined) time.
    PulseWidth,
    /// Rising (or falling, device-defined) edges counted over `gate`.
    EdgeCount,
}

/// One measurement with its time, see `PwmCaptureChannel::read_capture`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug)
)]
pub struct CaptureSample {
    /// End of the measurement window.
    pub t: Second,
    pub value: CaptureValue,
}

/// A measurement, in the mode [CaptureConfig::mode] set; the variant always matches it.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug),
    ww_repr = unib32
)]
pub enum CaptureValue {
    Frequency(Hertz),
    /// 0..=65535 = 0..100 %.
    DutyCycle(u16),
    PulseWidth(Second),
    EdgeCount(u32),
}

/// What a device's PWM offers, see `Pwm::capabilities`.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct PwmCapabilities<'i> {
    /// Channel `n` of every method is `channels[n]`.
    pub channels: MaxLen<RefVec<'i, PwmChannelInfo<'i>>, MAX_CHANNELS>,
    /// Lowest frequency any channel can be set to.
    pub min_freq: Hertz,
    /// Highest frequency any channel can be set to.
    pub max_freq: Hertz,
    /// Duty resolution of the hardware: `set_duty`'s 0..=65535 range is rounded to this many bits.
    pub duty_bits: u8,
}

/// One PWM channel, see [PwmCapabilities::channels].
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct PwmChannelInfo<'i> {
    /// For the user: "TIM1_CH1", "servo", "fan".
    pub name: MaxLen<&'i str, MAX_NAME_LEN>,
    /// Channels with the same group share a timer and so a frequency, see the crate docs.
    pub group: u8,
    /// This channel is also in `PwmCapture`, at the same index.
    pub capture: bool,
}

/// Errors of every trait in this crate: the common ones first, then what is specific to PWM. Each method lists the
/// variants it returns.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Eq, Clone),
    ww_repr = unib32,
    display
)]
pub enum PwmError<'i> {
    /// {0}
    #[common]
    Common(CommonError<'i>),
    /// The other channel of the group runs at another frequency.
    SharedTimer,
}

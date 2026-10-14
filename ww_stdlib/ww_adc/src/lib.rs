//! Analog inputs over WireWeaver: single calibrated readings, continuous sampling, and lossless captures into device
//! RAM.
//!
//! # Traits
//!
//! | Trait          | What it is for                                                         | Who implements it          |
//! |----------------|------------------------------------------------------------------------|----------------------------|
//! | `Adc`          | The channels, their unit and range; one calibrated reading on demand.  | Every device with an ADC   |
//! | `AdcStream`    | Continuous sampling of chosen channels, streamed while the link keeps up. | Data loggers, adapters   |
//! | `AdcCapture`   | A block of samples into device RAM, optionally triggered, read after.  | Scope-like devices         |
//!
//! A thermometer or a battery monitor implements `Adc` only; a data logger adds `AdcStream`; a device that samples
//! faster than its link (500 kS/s over USB full speed) adds `AdcCapture`. Traits a device doesn't put into its API
//! tree cost it nothing.
//!
//! ```rust,ignore
//! #[ww_api_root]
//! trait MyLogger {
//!     ww_impl!(adc: ww_adc::Adc);
//!     ww_impl!(adc_stream: ww_adc::AdcStream);
//! }
//! ```
//!
//! # Channels and units
//!
//! Channels are numbered `0..n` in [AdcCapabilities::channels], the order the device lists them. Each channel has a
//! physical unit and SI prefix ([ChannelInfo::unit], [ChannelInfo::prefix]): a voltage input reads in V or mV, a
//! current sense input in mA, the die temperature in °C. [Reading::value] is a number in that unit, in whatever
//! number type the device finds convenient (an `f32` after calibration, an `i32` of µV), so a host shows "3.297 V"
//! without knowing the device.
//!
//! `Adc::read` returns calibrated values. Streams and captures carry **raw** samples, to keep them compact; a host
//! converts them with [ChannelInfo::raw_to_value] (`value = raw * scale + offset`).
//!
//! # Simultaneous readings
//!
//! Readings of several channels taken at the same instant (three phase currents) are a capture of one sample set:
//! `AdcCapture::capture` with `sets: 1` and [Trigger::Now], if [CaptureCapabilities::simultaneous] says the
//! hardware samples the channels together. Several `Adc::read`s, even in one multi-call, are taken one after the
//! other.
//!
//! # Streams
//!
//! `AdcStream::samples` is a counted stream function: the host opens it with a [StreamConfig] and gets the rate
//! actually set back, closing it stops sampling. The unit of its positions is a sample set (one sample of every
//! channel), so the host knows exactly how many sets the device dropped and where, and each block may carry the
//! device time of its first set; a [SampleBlock] itself is pure data. Opening it again starts a new session with the
//! new configuration. See `docs/api/stream_reliability.md` for positions, losses and timestamps.
//!
//! # Errors
//!
//! All traits return [AdcError]; each method lists the variants it returns.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_numeric::NumericValue;
#[cfg(feature = "std")]
pub use ww_si::BaseUnitOwned;
pub use ww_si::{BaseUnit, Hertz, Prefix, Second};

/// Most channels a device lists, and most channels in one stream or capture.
pub const MAX_CHANNELS: usize = 32;

/// Longest channel name ([ChannelInfo::name]).
pub const MAX_NAME_LEN: usize = 32;

/// Largest sample data of one [SampleBlock], in bytes.
pub const MAX_BLOCK_LEN: usize = 512;

/// Largest piece of one `AdcCapture::capture_read`, in bytes.
pub const MAX_CAPTURE_READ_LEN: usize = 768;

/// Most sample formats a device lists.
pub const MAX_FORMATS: usize = 8;

/// The channels and single readings.
#[ww_trait]
pub trait Adc {
    /// One calibrated reading of `channel`, in the channel's unit.
    ///
    /// # Errors
    ///
    /// [AdcError::NoSuchChannel], [AdcError::Busy] (the ADC runs a stream or capture that can't be interrupted),
    /// [AdcError::NoIoLevel] (inputs behind an unpowered buffer), [AdcError::Timeout].
    fn read(channel: u8) -> Result<Reading, AdcError>;

    /// The channels, their units and ranges.
    fn capabilities() -> AdcCapabilities<'i>;
}

/// Continuous sampling, sent on the `samples` stream.
///
/// Only one stream runs at a time: opening `samples` again restarts it with the new configuration.
#[ww_trait]
pub trait AdcStream {
    /// Raw samples of `config.channels` at `config.rate` sample sets per second, whole sample sets interleaved in
    /// the order of [StreamConfig::channels]. Opening returns what was actually set (the nearest rate the hardware
    /// can do), closing stops sampling (blocks already sampled are still sent). Unit: sample set.
    ///
    /// # Errors
    ///
    /// [AdcError::NoSuchChannel], [AdcError::InvalidArgument] (rate or channel count out of range),
    /// [AdcError::Unsupported] (format), [AdcError::Busy] (a capture runs), [AdcError::InUse], [AdcError::NoIoLevel].
    #[stream(SampleBlock<'i>, counted)]
    fn samples(config: StreamConfig<'i>) -> Result<StreamActual, AdcError>;

    /// Rates, formats and limits of streaming.
    fn stream_capabilities() -> StreamCapabilities<'i>;
}

/// Lossless capture: samples go into device RAM at any rate the hardware can do, the host reads them out after.
#[ww_trait]
pub trait AdcCapture {
    /// Rates, depth, triggers and formats of captures.
    fn capture_capabilities() -> CaptureCapabilities<'i>;

    /// Arms a capture. With [Trigger::Now] it starts at once; otherwise it samples `pre_trigger` sets continuously
    /// and keeps them, waits for the trigger, then takes the rest. Replaces a previous capture and its data.
    ///
    /// # Errors
    ///
    /// [AdcError::NoSuchChannel], [AdcError::InvalidArgument] (rate, depth or trigger level out of range),
    /// [AdcError::Unsupported] (format or trigger), [AdcError::Busy] (a stream runs), [AdcError::InUse],
    /// [AdcError::NoIoLevel].
    fn capture(config: CaptureConfig<'i>) -> Result<CaptureActual, AdcError>;

    /// Where the capture stands.
    fn capture_status() -> CaptureStatus;

    /// Reads `len` bytes of the captured data from `offset`. The data is raw samples, interleaved in the order of
    /// [CaptureConfig::channels], `CaptureActual::bytes` long; the first `pre_trigger` sets come before the trigger.
    ///
    /// # Errors
    ///
    /// [AdcError::WrongMode] (no finished capture), [AdcError::InvalidArgument] (outside the data).
    fn capture_read(
        offset: u32,
        len: u16,
    ) -> Result<MaxLen<RefVec<'i, u8>, MAX_CAPTURE_READ_LEN>, AdcError>;

    /// Stops an armed or running capture and drops its data.
    fn capture_abort();
}

/// One calibrated reading, see `Adc::read`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy)
)]
pub struct Reading {
    /// In the channel's [ChannelInfo::unit] and [ChannelInfo::prefix].
    pub value: NumericValue,
    /// The converter's raw code (for checking calibration).
    pub raw: u32,
    /// When the sample was taken.
    pub t: Second,
}

/// What a device's ADC offers, see `Adc::capabilities`.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct AdcCapabilities<'i> {
    /// Channel `n` of every method is `channels[n]`.
    pub channels: MaxLen<RefVec<'i, ChannelInfo<'i>>, MAX_CHANNELS>,
}

/// One analog input.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct ChannelInfo<'i> {
    /// For the user: "IO0", "VBUS", "die temperature".
    pub name: MaxLen<&'i str, MAX_NAME_LEN>,
    /// Physical unit of [Reading::value], [ChannelInfo::min] and [ChannelInfo::max].
    pub unit: BaseUnit<'i>,
    /// SI prefix of the values.
    pub prefix: Prefix,
    /// Lowest value the channel can show.
    pub min: NumericValue,
    /// Highest value the channel can show.
    pub max: NumericValue,
    /// Converter resolution.
    pub bits: u8,
    /// Whether readings are calibrated for this unit (per-device factors) or nominal only.
    pub calibrated: bool,
    /// How raw samples of streams and captures turn into values; `None` if they don't linearly.
    pub raw_to_value: Option<Linear>,
}

/// `value = raw * scale + offset`, in the channel's unit and prefix.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy, PartialEq)
)]
pub struct Linear {
    pub scale: f32,
    pub offset: f32,
}

/// How raw samples are packed in [SampleBlock::data] and capture data.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum SampleFormat {
    /// One byte per sample.
    U8,
    /// Two bytes per sample, little-endian.
    U16,
    /// Two 12-bit samples in three bytes: `a[7:0]`, `a[11:8] | b[3:0] << 4`, `b[11:4]`; an odd last sample takes
    /// two bytes.
    Packed12,
    /// Two bytes per sample, signed, little-endian (differential inputs).
    I16,
    /// Three bytes per sample, signed, little-endian (sigma-delta converters).
    I24,
}

/// What `AdcStream` can do.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct StreamCapabilities<'i> {
    /// Highest sample rate of all channels together (samples, not sets, per second), alone on the link.
    pub max_total_rate: Hertz,
    /// Most channels in one stream.
    pub max_channels: u8,
    pub formats: MaxLen<RefVec<'i, SampleFormat>, MAX_FORMATS>,
}

/// What to stream, see `AdcStream::samples`.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct StreamConfig<'i> {
    /// Channel indices, in the order samples are interleaved.
    pub channels: MaxLen<RefVec<'i, u8>, MAX_CHANNELS>,
    /// Sample sets per second.
    pub rate: Hertz,
    pub format: SampleFormat,
    /// Send a block at least this often even if it isn't full (small rates would wait long otherwise); `None` lets
    /// the device choose (around 10 ms).
    pub flush_interval: Option<Second>,
}

/// What opening `AdcStream::samples` actually set.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy)
)]
pub struct StreamActual {
    /// Sample sets per second.
    pub rate: Hertz,
}

/// Raw samples of a stream, see `AdcStream::samples`; its position and time are in the event's head.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct SampleBlock<'i> {
    /// Whole sample sets, interleaved in the order of [StreamConfig::channels], packed as [StreamConfig::format].
    pub data: MaxLen<RefVec<'i, u8>, MAX_BLOCK_LEN>,
}

/// What `AdcCapture` can do.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct CaptureCapabilities<'i> {
    /// Highest sample rate of all channels together (samples per second).
    pub max_total_rate: Hertz,
    /// Capture memory in bytes.
    pub max_bytes: u32,
    /// Whether the channels of one sample set are sampled at the same instant (simultaneous sampling ADCs, or
    /// several ADCs triggered together).
    pub simultaneous: bool,
    /// Which [Trigger]s work.
    pub level_trigger: bool,
    pub external_trigger: bool,
    pub formats: MaxLen<RefVec<'i, SampleFormat>, MAX_FORMATS>,
}

/// What to capture, see `AdcCapture::capture`.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct CaptureConfig<'i> {
    /// Channel indices, in the order samples are interleaved.
    pub channels: MaxLen<RefVec<'i, u8>, MAX_CHANNELS>,
    /// Sample sets per second.
    pub rate: Hertz,
    /// Sample sets to keep in total, `pre_trigger` included.
    pub sets: u32,
    /// Sample sets to keep from before the trigger.
    pub pre_trigger: u32,
    pub trigger: Trigger,
    pub format: SampleFormat,
}

/// When a capture starts.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy),
    ww_repr = unib32
)]
pub enum Trigger {
    /// Right away.
    Now,
    /// When `channel` rises through `level` (in the channel's unit and prefix).
    Rising { channel: u8, level: NumericValue },
    /// When `channel` falls through `level`.
    Falling { channel: u8, level: NumericValue },
    /// On the device's external trigger input, or another device's trigger.
    External,
}

/// What `AdcCapture::capture` actually set.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy)
)]
pub struct CaptureActual {
    /// Sample sets per second.
    pub rate: Hertz,
    /// Size of the data once done, for `AdcCapture::capture_read`.
    pub bytes: u32,
}

/// Where a capture stands, see `AdcCapture::capture_status`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy),
    ww_repr = unib32
)]
pub enum CaptureStatus {
    /// Nothing armed, no data.
    Idle,
    /// Waiting for the trigger (sampling the pre-trigger part).
    Armed,
    /// Triggered at `t`, taking the rest.
    Triggered { t: Second },
    /// Done: data ready to read. `t_first` is the time of the first set, `t_trigger` the trigger's.
    Done {
        t_first: Second,
        t_trigger: Option<Second>,
        sets: u32,
    },
}

/// Errors of every trait in this crate. Each method lists the variants it returns.
///
/// How errors evolve (variants from newer versions, per-method error lists, errors that carry a reason) is
/// still open for `ww_stdlib`; until then this enum stays plain.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Eq, Copy, Clone),
    self_describing,
    ww_repr = unib32
)]
pub enum AdcError {
    /// Not supported by this device (a format, a trigger).
    Unsupported,
    /// The pins or the converter serve another function.
    InUse,
    /// The input buffers or translators have no supply.
    NoIoLevel,
    /// A safety rule of the device doesn't allow it.
    Refused,
    /// An argument is out of range (rate, depth, offset, trigger level).
    InvalidArgument,
    /// The call needs another state (e.g. reading a capture that isn't done).
    WrongMode,
    /// No channel with this index.
    NoSuchChannel,
    /// The converter is taken by a stream or capture.
    Busy,
    /// The conversion didn't finish.
    Timeout,
}

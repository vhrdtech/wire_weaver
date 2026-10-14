//! Logic analyzers over WireWeaver: many digital channels sampled at one rate, captured into device RAM or
//! streamed, in a raw or a transition encoding.
//!
//! # Traits
//!
//! | Trait           | What it is for                                                              | Who implements it |
//! |-----------------|-----------------------------------------------------------------------------|-------------------|
//! | `LogicAnalyzer` | Triggered capture into device RAM, read out after; lossless at any rate.    | Every analyzer    |
//! | `LogicStream`   | Continuous capture streamed while the link keeps up.                        | Fast links, sparse signals |
//!
//! ```rust,ignore
//! #[ww_api_root]
//! trait MyProbe {
//!     ww_impl!(la: ww_logic_analyzer::LogicAnalyzer);
//! }
//! ```
//!
//! Protocol decoding stays on the host; a device that decodes a protocol itself implements that protocol's monitor
//! trait (`I2cMonitor`, `UartMonitor`, `SpiMonitor`). See `docs/std_library/ww_logic_analyzer.md` for the encodings
//! and worked examples.
//!
//! # Encodings
//!
//! - [Encoding::Raw], [RawChunk]: every sample, a word of [RawLayout::width] bits; bit `i` of a word is the `i`-th
//!   channel of the configuration's channel list. Sub-byte words are packed starting at bit 0 of a byte (sample 0
//!   in the lowest bits), multi-byte words are little-endian. What a DMA from a GPIO port produces.
//! - [Encoding::Transitions], [TransitionsChunk]: per channel, its level at the start and the sample indices where
//!   it toggles, as a `DeltaOfDelta<u32>` (SW-31). A steady clock costs about one bit per edge, a quiet channel
//!   almost nothing, so the edge rate limits a stream, not the sample rate.
//!
//! Sample indices are `u32` counted from a capture's first sample, or from the start of a stream (where they wrap
//! and the host unwraps them).
//!
//! # Time
//!
//! Timestamps are device time ([Second]) of the device's clock (`ww_timebase::Clock`); sample `k` of a capture was
//! taken at `t_first + k / rate`, with the rate the device actually set. A stream's chunks carry the time of their
//! first sample in the event head instead.
//!
//! # Errors
//!
//! Both traits return [LaError]; each method lists the variants it returns.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_gpio::Level;
pub use ww_i2c::I2cAddress;
pub use ww_si::{Hertz, Second, Volt};

/// Most channels a device lists, and most channels in one capture or stream.
pub const MAX_CHANNELS: usize = 32;

/// Longest channel name ([ChannelInfo::name]).
pub const MAX_NAME_LEN: usize = 32;

/// Largest raw data of one [RawChunk], in bytes; the worst case of a [TransitionsChunk] (128 edges) fits as well.
pub const MAX_READ_LEN: usize = 768;

/// Most edges of one [TransitionsChunk]: 128 indices take at most 592 bytes, so a chunk fits a reply.
pub const MAX_EDGES: usize = 128;

/// Most trigger stages of one [Trigger::Stages].
pub const MAX_STAGES: usize = 8;

/// Most entries of [LaCapabilities::rates].
pub const MAX_RATE_LIMITS: usize = 8;

/// Triggered capture into device RAM, read out after.
///
/// Same shape as `ww_adc::AdcCapture`: arm with a configuration, poll the status, read the data out, abort.
#[ww_trait]
pub trait LogicAnalyzer {
    /// Channels, rates, memory, encodings and triggers.
    fn capabilities() -> LaCapabilities<'i>;

    /// Arms a capture, replacing the previous one and its data, and returns what was actually set. With
    /// [Trigger::Now] it starts at once; otherwise it samples `pre_trigger` samples continuously and keeps them,
    /// waits for the trigger, then takes the rest.
    ///
    /// # Errors
    ///
    /// [LaError::NoSuchChannel], [LaError::InvalidArgument] (rate, depth, threshold or trigger out of range),
    /// [LaError::Unsupported] (a trigger stage or condition), [LaError::Busy] (a stream runs), [LaError::InUse],
    /// [LaError::NoIoLevel].
    fn capture(config: CaptureConfig<'i>) -> Result<CaptureActual, LaError>;

    /// Arms again with the configuration of the last `capture`, for repeated captures of the same event.
    ///
    /// # Errors
    ///
    /// [LaError::WrongMode] (nothing was configured yet), and those of `capture`.
    fn rearm() -> Result<CaptureActual, LaError>;

    /// Where the capture stands.
    fn status() -> CaptureStatus;

    /// Samples of `range` of a finished capture as raw words, at most [MAX_READ_LEN] bytes: the chunk may hold
    /// fewer samples than asked, a whole number of words.
    ///
    /// # Errors
    ///
    /// [LaError::WrongMode] (no finished capture), [LaError::InvalidArgument] (outside the capture),
    /// [LaError::Unsupported] (the device doesn't give raw words).
    fn read_raw(range: SampleRange) -> Result<RawChunk<'i>, LaError>;

    /// Edges of `channel` (an index into [CaptureConfig::channels]) from sample `from` on, up to [MAX_EDGES]
    /// edges or a full reply; [TransitionsChunk::scanned_to] says where the next call starts.
    ///
    /// # Errors
    ///
    /// [LaError::WrongMode] (no finished capture), [LaError::InvalidArgument] (outside the capture),
    /// [LaError::NoSuchChannel], [LaError::Unsupported] (the device doesn't give transitions).
    fn read_transitions(channel: u8, from: u32) -> Result<TransitionsChunk<'i>, LaError>;

    /// Stops an armed or running capture and drops its data.
    fn abort();
}

/// Continuous capture, sent on the `data` stream.
///
/// Opening `data` starts sampling with its configuration, closing it stops; opening it again restarts it with the
/// new one. Every chunk carries its own sample range, and the stream is counted: the host knows exactly which chunks
/// the device dropped, and each chunk may carry the device time of its first sample in the event head.
#[ww_trait]
pub trait LogicStream {
    /// Captured data in `config`'s encoding: raw chunks, or transition chunks of each channel as they fill. Opening
    /// returns what was actually set. Unit: chunk.
    ///
    /// # Errors
    ///
    /// [LaError::NoSuchChannel], [LaError::InvalidArgument] (rate, channel count or threshold out of range),
    /// [LaError::Unsupported] (encoding), [LaError::Busy] (a capture runs), [LaError::InUse], [LaError::NoIoLevel].
    #[stream(StreamChunk<'i>, counted)]
    fn data(config: StreamConfig<'i>) -> Result<StreamActual, LaError>;

    /// Rates, channels and encodings of streaming.
    fn stream_capabilities() -> StreamCapabilities<'i>;
}

/// What a device's analyzer offers, see `LogicAnalyzer::capabilities`.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct LaCapabilities<'i> {
    /// Channel `n` of every configuration is `channels[n]`.
    pub channels: MaxLen<RefVec<'i, ChannelInfo<'i>>, MAX_CHANNELS>,
    /// Highest sample rate per channel count, ascending by count.
    pub rates: MaxLen<RefVec<'i, RateLimit>, MAX_RATE_LIMITS>,
    /// Capture memory in bytes.
    pub memory: u32,
    /// How captures are kept in RAM.
    pub storage: Encoding,
    /// Encodings the read methods give (either storage can be read in either encoding the device lists).
    pub raw: bool,
    pub transitions: bool,
    pub trigger: TriggerCapabilities,
}

/// One input.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct ChannelInfo<'i> {
    /// For the user: "D0", "CLK", "PA5".
    pub name: MaxLen<&'i str, MAX_NAME_LEN>,
    /// The same pin's index in the device's `ww_gpio::Bank`, if it has one.
    pub gpio_pin: Option<u8>,
    /// Range of [CaptureConfig::threshold], if the threshold is adjustable.
    pub threshold: Option<VoltRange>,
}

/// `min..=max`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy)
)]
pub struct VoltRange {
    pub min: Volt,
    pub max: Volt,
}

/// Up to `channels` channels sample at up to `max_rate`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy)
)]
pub struct RateLimit {
    pub channels: u8,
    pub max_rate: Hertz,
}

/// Which triggers work.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy)
)]
pub struct TriggerCapabilities {
    /// Most stages of [Trigger::Stages] (0: no stages, only [Trigger::Now] and maybe [Trigger::External]).
    pub max_stages: u8,
    /// Most conditions of one [TriggerStage::Pattern].
    pub max_conditions: u8,
    /// [Condition::Rising], [Condition::Falling], [Condition::AnyEdge] work (levels always do).
    pub edges: bool,
    pub uart_byte: bool,
    pub i2c_address: bool,
    pub spi_first_byte: bool,
    /// [Trigger::External] works.
    pub external: bool,
    /// [CaptureConfig::trigger_out] works.
    pub trigger_out: bool,
}

/// What to capture, see `LogicAnalyzer::capture`.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct CaptureConfig<'i> {
    /// Channel numbers ([LaCapabilities::channels]), in the bit order of raw words.
    pub channels: MaxLen<RefVec<'i, u8>, MAX_CHANNELS>,
    /// Samples per second.
    pub rate: Hertz,
    /// Samples to keep in total, `pre_trigger` included.
    pub samples: u32,
    /// Samples to keep from before the trigger.
    pub pre_trigger: u32,
    pub trigger: Trigger<'i>,
    /// Input threshold where it is adjustable ([ChannelInfo::threshold]); `None` leaves it.
    pub threshold: Option<Volt>,
    /// Pulse the trigger output when the capture triggers.
    pub trigger_out: bool,
}

/// What `LogicAnalyzer::capture` actually set.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy)
)]
pub struct CaptureActual {
    /// Samples per second.
    pub rate: Hertz,
    /// Samples the capture will hold at most (the memory may hold fewer than asked at this channel count).
    pub samples: u32,
    /// Layout of [RawChunk::data].
    pub layout: RawLayout,
}

/// When a capture triggers.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone),
    ww_repr = unib32
)]
pub enum Trigger<'i> {
    /// Right away.
    Now,
    /// Stage after stage, each one met after the one before; the capture triggers when the last one is met.
    Stages(MaxLen<RefVec<'i, TriggerStage<'i>>, MAX_STAGES>),
    /// On the external trigger input.
    External { edge: Edge },
}

/// One step of [Trigger::Stages]. Protocol stages are matched on the decoded stream next to the capture, decoding
/// only as much as the match needs.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone),
    ww_repr = unib32
)]
pub enum TriggerStage<'i> {
    /// Every condition holds on the same sample: a level, an edge, a pattern, or an edge qualified by levels.
    Pattern(MaxLen<RefVec<'i, ChannelCondition>, MAX_CHANNELS>),
    /// A UART frame (8 data bits, LSB first, idle high) on `channel` at `baud` whose data matches `value` under
    /// `mask`; met at the stop bit.
    UartByte {
        channel: u8,
        baud: u32,
        value: u8,
        mask: u8,
    },
    /// An I2C address byte on `scl` / `sda` matching `address`, of a read (`Some(true)`), a write (`Some(false)`)
    /// or either (`None`); met at the ACK bit.
    I2cAddress {
        scl: u8,
        sda: u8,
        address: I2cAddress,
        read: Option<bool>,
    },
    /// The first byte on `data` (MOSI or MISO) after `cs` goes low, clocked by `sck` in SPI mode `mode` (0..=3),
    /// matching `value` under `mask`; met at its last bit.
    SpiFirstByte {
        cs: u8,
        sck: u8,
        data: u8,
        mode: u8,
        msb_first: bool,
        value: u8,
        mask: u8,
    },
}

/// `channel` (an index into [CaptureConfig::channels]) meets `when`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy, PartialEq, Eq)
)]
pub struct ChannelCondition {
    pub channel: u8,
    pub when: Condition,
}

/// What a channel does on the matching sample.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum Condition {
    Low,
    High,
    Rising,
    Falling,
    AnyEdge,
}

/// Which edge of an input.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum Edge {
    Rising,
    Falling,
    Any,
}

/// Where a capture stands, see `LogicAnalyzer::status`.
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
    /// Sampling the pre-trigger part, waiting for the trigger.
    Armed,
    /// Triggered at `t`, taking the rest.
    Triggered { t: Second },
    /// Done: sample 0 was taken at `t_first`, the trigger fired at sample `trigger_at` (`None` for
    /// [Trigger::Now]). `samples` may be fewer than asked when transition storage filled up.
    Done {
        t_first: Second,
        trigger_at: Option<u32>,
        samples: u32,
    },
}

/// `count` samples from `first`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy, PartialEq, Eq)
)]
pub struct SampleRange {
    pub first: u32,
    pub count: u32,
}

/// The two encodings, see the crate docs.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum Encoding {
    /// Every sample, bit-packed ([RawChunk]).
    Raw,
    /// Edges per channel ([TransitionsChunk]).
    Transitions,
}

/// `width` bits per sample: 1, 2, 4, 8, 16 or 32, the smallest the device gives for the channel count.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy, PartialEq, Eq)
)]
pub struct RawLayout {
    pub width: u8,
}

/// A run of raw samples, see [Encoding::Raw].
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct RawChunk<'i> {
    /// Index of the first sample.
    pub first: u32,
    pub samples: u32,
    /// `samples` words as in [RawLayout].
    pub data: MaxLen<RefVec<'i, u8>, MAX_READ_LEN>,
}

/// Edges of one channel in `from..scanned_to`, see [Encoding::Transitions].
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct TransitionsChunk<'i> {
    /// Index into the configuration's channel list.
    pub channel: u8,
    /// Level at sample `from`.
    pub level: Level,
    pub from: u32,
    /// No more edges before this sample; the next chunk of this channel starts here.
    pub scanned_to: u32,
    /// Samples where the channel toggles, ascending, in `from..scanned_to`; at most [MAX_EDGES].
    pub edges: DeltaOfDelta<'i, u32>,
}

/// One item of the `LogicStream::data` stream.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone),
    ww_repr = unib32
)]
pub enum StreamChunk<'i> {
    Raw(RawChunk<'i>),
    Transitions(TransitionsChunk<'i>),
}

/// What `LogicStream` can do.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct StreamCapabilities<'i> {
    /// Highest raw sample rate per channel count the link carries, alone on it.
    pub raw_rates: MaxLen<RefVec<'i, RateLimit>, MAX_RATE_LIMITS>,
    /// Highest sample rate with [Encoding::Transitions], as long as the edge rate fits the link.
    pub transitions_rate: Option<Hertz>,
}

/// What to stream, see `LogicStream::data`.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct StreamConfig<'i> {
    /// Channel numbers ([LaCapabilities::channels]), in the bit order of raw words.
    pub channels: MaxLen<RefVec<'i, u8>, MAX_CHANNELS>,
    /// Samples per second.
    pub rate: Hertz,
    pub encoding: Encoding,
    /// Send a chunk at least this often even if it isn't full; `None` lets the device choose.
    pub flush_interval: Option<Second>,
    /// Input threshold where it is adjustable; `None` leaves it.
    pub threshold: Option<Volt>,
}

/// What opening `LogicStream::data` actually set.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy)
)]
pub struct StreamActual {
    /// Samples per second.
    pub rate: Hertz,
    /// Layout of raw chunks.
    pub layout: RawLayout,
}

/// Errors of both traits in this crate. Each method lists the variants it returns.
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
pub enum LaError {
    /// Not supported by this device (an encoding, a trigger stage).
    Unsupported,
    /// The pins serve another function in a way sampling can't share (a threshold, a pull).
    InUse,
    /// The input buffers or translators have no supply.
    NoIoLevel,
    /// A safety rule of the device doesn't allow it.
    Refused,
    /// An argument is out of range (rate, depth, range, threshold).
    InvalidArgument,
    /// The call needs another state (reading a capture that isn't done, `rearm` before any `capture`).
    WrongMode,
    /// No channel with this index.
    NoSuchChannel,
    /// The sampler is taken by a stream or a capture.
    Busy,
}

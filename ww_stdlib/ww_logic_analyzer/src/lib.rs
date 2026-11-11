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
//! (`I2cMonitor`, `Uart::monitor`, `Spi::monitor`). See `docs/std_library/ww_logic_analyzer.md` for the encodings
//! and worked examples.
//!
//! # Encodings
//!
//! - [Encoding::Raw], [RawChunk]: every sample, a word of [RawLayout::width] bits; bit `i` of a word is the `i`-th
//!   channel of the configuration's channel list. Sub-byte words are packed starting at bit 0 of a byte (sample 0
//!   in the lowest bits), multi-byte words are little-endian. What a DMA from a GPIO port produces.
//! - [Encoding::Transitions], [TransitionsChunk]: per channel, its level at the start and the sample indices where
//!   it toggles, as a `Series<Index<Sample, u32>>` (SW-60, its numeric column picks delta of delta for a clock). A
//!   steady clock costs a few bytes per chunk, a quiet channel almost nothing, so the edge rate limits a stream, not
//!   the sample rate.
//!
//! Sample indices are [Index]`<Sample, u32>` counted from a capture's first sample, or from the start of a stream
//! (where they wrap and the host unwraps them).
//!
//! # Time
//!
//! Timestamps are device time ([Second]) of the device's clock (`ww_timebase::Clock`); sample `k` of a capture was
//! taken at `t_first + k / rate`, with the rate the device actually set. A stream's chunks carry the time of their
//! first sample in the event head instead.
//!
//! # Errors
//!
//! Both traits return [LaError]: the framework's `CommonError` as [LaError::Common], plus `NoSuchChannel`. Each
//! method lists the variants it returns (`#[errors]`).
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_gpio::Level;
pub use ww_i2c::I2cAddress;
pub use ww_si::prefix::One;
pub use ww_si::units::{Bit, Channel, Item, Pin, Sample};
pub use ww_si::{Baud, Bytes, Count, Fixed, Hertz, Index, Second, Volt};

/// Most channels a device lists, and most channels in one capture or stream.
pub const MAX_CHANNELS: usize = 32;

/// Longest channel name ([ChannelInfo::name]).
pub const MAX_NAME_LEN: usize = 32;

/// Largest raw data of one [RawChunk], in bytes; the worst case of a [TransitionsChunk] (128 edges) fits as well.
pub const MAX_READ_LEN: usize = 768;

/// Most edges of one [TransitionsChunk]: 128 indices take at most 610 bytes (`Series::<Index<Sample, u32>>::max_bytes(128)`,
/// the same bytes as `u32`, [Index] being transparent), so a chunk fits a reply.
pub const MAX_EDGES: usize = 128;

// the worst case of a chunk's edges fits a reply
const _: () = assert!(
    matches!(Series::<'static, Index<Sample, u32>>::max_bytes(MAX_EDGES), Some(n) if n <= MAX_READ_LEN)
);

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
    /// `InvalidArgument`: rate, depth, threshold or trigger out of range; `Unsupported`: a trigger stage or condition;
    /// `Busy`: a stream runs.
    #[errors(
        Common(InvalidArgument, Unsupported, Busy, InUse, NoPower),
        NoSuchChannel
    )]
    fn capture(config: CaptureConfig<'i>) -> Result<CaptureActual, LaError<'i>>;

    /// Arms again with the configuration of the last `capture`, for repeated captures of the same event.
    /// `WrongState`: nothing was configured yet.
    #[errors(Common(WrongState))]
    fn rearm() -> Result<CaptureActual, LaError<'i>>;

    /// Where the capture stands.
    fn status() -> CaptureStatus;

    /// Samples of `range` of a finished capture as raw words, at most [MAX_READ_LEN] bytes: the chunk may hold
    /// fewer samples than asked, a whole number of words.
    /// `WrongState`: no finished capture; `InvalidArgument`: outside the capture; `Unsupported`: the device doesn't
    /// give raw words.
    #[errors(Common(WrongState, InvalidArgument, Unsupported))]
    fn read_raw(range: SampleRange) -> Result<RawChunk<'i>, LaError<'i>>;

    /// Edges of `channel` (an index into [CaptureConfig::channels]) from sample `from` on, up to [MAX_EDGES]
    /// edges or a full reply; [TransitionsChunk::scanned_to] says where the next call starts.
    /// `WrongState`: no finished capture; `InvalidArgument`: outside the capture; `Unsupported`: the device doesn't
    /// give transitions.
    #[errors(Common(WrongState, InvalidArgument, Unsupported), NoSuchChannel)]
    fn read_transitions(
        channel: Index<Channel, u8>,
        from: Index<Sample, u32>,
    ) -> Result<TransitionsChunk<'i>, LaError<'i>>;

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
    /// returns what was actually set. Unit: chunk. `InvalidArgument` for a rate, channel count or threshold out of
    /// range, `Unsupported` for the encoding, `Busy` while a capture runs.
    #[errors(
        NoSuchChannel,
        Common(InvalidArgument, Unsupported, Busy, InUse, NoPower)
    )]
    #[stream(StreamChunk<'i>, counted)]
    fn data(config: StreamConfig<'i>) -> Result<StreamActual, LaError<'i>>;

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
    /// Capture memory.
    pub memory: Bytes<u32>,
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
    pub gpio_pin: Option<Index<Pin, u8>>,
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
    pub channels: Count<Channel, u8>,
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
    pub max_stages: Count<Item, u8>,
    /// Most conditions of one [TriggerStage::Pattern].
    pub max_conditions: Count<Item, u8>,
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
    pub channels: MaxLen<RefVec<'i, Index<Channel, u8>>, MAX_CHANNELS>,
    /// Samples per second.
    pub rate: Hertz,
    /// Samples to keep in total, `pre_trigger` included.
    pub samples: Count<Sample, u32>,
    /// Samples to keep from before the trigger.
    pub pre_trigger: Count<Sample, u32>,
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
    pub samples: Count<Sample, u32>,
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
        channel: Index<Channel, u8>,
        baud: Baud,
        value: u8,
        mask: u8,
    },
    /// An I2C address byte on `scl` / `sda` matching `address`, of a read (`Some(true)`), a write (`Some(false)`)
    /// or either (`None`); met at the ACK bit.
    I2cAddress {
        scl: Index<Channel, u8>,
        sda: Index<Channel, u8>,
        address: I2cAddress,
        read: Option<bool>,
    },
    /// The first byte on `data` (MOSI or MISO) after `cs` goes low, clocked by `sck` in SPI `mode`, matching `value`
    /// under `mask`; met at its last bit.
    SpiFirstByte {
        cs: Index<Channel, u8>,
        sck: Index<Channel, u8>,
        data: Index<Channel, u8>,
        mode: SpiMode,
        msb_first: bool,
        value: u8,
        mask: u8,
    },
}

/// Clock polarity and phase (CPOL/CPHA), the usual SPI mode numbering. `ww_spi::SpiMode` says the same thing for a
/// device's own SPI bus; this crate keeps its own copy (`ww_spi::SpiMode` doesn't derive `defmt::Format`, needed
/// here since [TriggerStage] does) rather than a dependency on `ww_spi` for one enum.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    sized,
    ww_repr = u2
)]
pub enum SpiMode {
    /// CPOL = 0, CPHA = 0.
    Mode0,
    /// CPOL = 0, CPHA = 1.
    Mode1,
    /// CPOL = 1, CPHA = 0.
    Mode2,
    /// CPOL = 1, CPHA = 1.
    Mode3,
}

/// `channel` (an index into [CaptureConfig::channels]) meets `when`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy, PartialEq, Eq)
)]
pub struct ChannelCondition {
    pub channel: Index<Channel, u8>,
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

/// Which edge of an input. Complete by nature, so without an `Unknown` variant for newer versions.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy, PartialEq, Eq),
    ww_repr = unib32,
    no_future_version
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
        trigger_at: Option<Index<Sample, u32>>,
        samples: Count<Sample, u32>,
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
    pub first: Index<Sample, u32>,
    pub count: Count<Sample, u32>,
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
    pub width: Fixed<Bit, One, u8>,
}

/// A run of raw samples, see [Encoding::Raw].
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct RawChunk<'i> {
    /// Index of the first sample.
    pub first: Index<Sample, u32>,
    pub samples: Count<Sample, u32>,
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
    pub channel: Index<Channel, u8>,
    /// Level at sample `from`.
    pub level: Level,
    pub from: Index<Sample, u32>,
    /// No more edges before this sample; the next chunk of this channel starts here.
    pub scanned_to: Index<Sample, u32>,
    /// Samples where the channel toggles, ascending, in `from..scanned_to`; at most [MAX_EDGES].
    pub edges: Series<'i, Index<Sample, u32>>,
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
    pub channels: MaxLen<RefVec<'i, Index<Channel, u8>>, MAX_CHANNELS>,
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

/// Errors of both traits in this crate: the common ones first, then `NoSuchChannel`. Each method lists the variants
/// it returns.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Eq, Clone),
    ww_repr = unib32,
    display
)]
pub enum LaError<'i> {
    /// {0}
    #[common]
    Common(CommonError<'i>),
    /// No channel with this index.
    NoSuchChannel,
}

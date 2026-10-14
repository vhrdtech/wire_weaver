//! Signal generators over WireWeaver: digital patterns on many pins at a fixed rate, and waveforms on DACs.
//!
//! # Traits
//!
//! | Trait               | What it is for                                                          | Who implements it |
//! |---------------------|-------------------------------------------------------------------------|-------------------|
//! | `PatternGenerator`  | Digital channels driven from a pattern, once or repeated; replays analyzer captures. | MCUs with DMA-to-GPIO or a PIO |
//! | `WaveformGenerator` | `channel[]` of DAC outputs: sine, square, triangle, sawtooth, DC, tables. | Devices with a DAC |
//!
//! ```rust,ignore
//! #[ww_api_root]
//! trait MyBench {
//!     ww_impl!(la: ww_logic_analyzer::LogicAnalyzer);
//!     ww_impl!(pattern: ww_signal_generator::PatternGenerator);
//!     ww_impl!(dac: ww_signal_generator::WaveformGenerator);
//! }
//! ```
//!
//! # Replaying a capture
//!
//! Patterns are loaded in the encodings of `ww_logic_analyzer`: [StreamChunk] (re-exported here), raw words or
//! per-channel transitions. A host reads a capture in an encoding the generator accepts
//! ([PgCapabilities::raw], [PgCapabilities::transitions]) and sends the chunks to the `pattern` sink as they are.
//!
//! A square wave from a timer pin is `ww_pwm`; `WaveformGenerator` is for DACs. See
//! `docs/std_library/ww_signal_generator.md` for a full writeup.
//!
//! # Errors
//!
//! `PatternGenerator` returns [PgError], `WaveformGenerator` [WgError]; each method lists the variants it returns.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_adc::{Linear, SampleFormat};
pub use ww_logic_analyzer::{Edge, RateLimit, StreamChunk};
pub use ww_si::{Hertz, Second, Volt};

/// Most channels a device lists, and most channels in one pattern.
pub const MAX_CHANNELS: usize = 32;

/// Longest channel name.
pub const MAX_NAME_LEN: usize = 32;

/// Most entries of [PgCapabilities::rates].
pub const MAX_RATE_LIMITS: usize = 8;

/// Most sample formats a DAC channel lists.
pub const MAX_FORMATS: usize = 8;

/// Largest piece of a waveform table sent to the `table` sink, in bytes.
pub const MAX_TABLE_CHUNK: usize = 512;

/// Digital channels driven from a pattern at a fixed rate, all switching on the same tick.
///
/// `configure` sets it up, the `pattern` sink loads the data (`Open` starts a new pattern, `Close` completes it),
/// `start` plays it. While it runs its pins belong to it: `ww_gpio` calls that would drive them answer `InUse`.
#[ww_trait]
pub trait PatternGenerator {
    /// Channels, rates, memory, encodings and start conditions.
    fn capabilities() -> PgCapabilities<'i>;

    /// Sets channels, rate, repetition, idle levels and the start condition, and returns what was actually set.
    /// Drops a loaded pattern.
    ///
    /// # Errors
    ///
    /// [PgError::NoSuchChannel], [PgError::InvalidArgument] (rate or channel count out of range),
    /// [PgError::Unsupported] (start condition), [PgError::Busy] (a pattern plays), [PgError::InUse],
    /// [PgError::NoIoLevel].
    fn configure(config: PgConfig<'i>) -> Result<PgActual, PgError>;

    /// Pattern data, appended in order. The credit is the free pattern memory.
    #[credit]
    sink!(pattern: StreamChunk<'i>);

    /// Starts output now, or waits for [PgConfig::start]. Started before the pattern is complete, it plays what it
    /// has while the sink keeps filling, and stops with [PgStatus::Underrun] if the host falls behind.
    ///
    /// # Errors
    ///
    /// [PgError::WrongMode] (not configured, or nothing loaded), [PgError::Refused].
    fn start() -> Result<(), PgError>;

    /// Stops output; the channels go to their idle levels.
    fn stop();

    /// Where the generator stands.
    fn status() -> PgStatus;
}

/// DAC outputs.
#[ww_trait]
pub trait WaveformGenerator {
    ww_impl!(channel[]: WaveformChannel);

    /// The channels, their ranges and formats.
    fn capabilities() -> WgCapabilities<'i>;

    /// Starts every channel in `channels` (already `set`, held) on the same sample clock tick, so their phases line
    /// up.
    ///
    /// # Errors
    ///
    /// [WgError::NoSuchChannel], [WgError::Unsupported] (the channels don't share a clock), [WgError::WrongMode].
    fn start_together(channels: MaxLen<RefVec<'i, u8>, MAX_CHANNELS>) -> Result<(), WgError>;
}

/// One DAC output.
#[ww_trait]
pub trait WaveformChannel {
    /// Starts or changes the output and returns what the DAC actually does.
    ///
    /// # Errors
    ///
    /// [WgError::InvalidArgument] (frequency or levels out of range), [WgError::Unsupported] (shape or format),
    /// [WgError::WrongMode] (an `Arbitrary` shape with no table), [WgError::InUse], [WgError::Refused].
    fn set(waveform: Waveform) -> Result<WaveformActual, WgError>;

    /// Stops and holds `level`; returns the level actually held.
    ///
    /// # Errors
    ///
    /// [WgError::InvalidArgument] (out of range), [WgError::Refused].
    fn stop(level: Volt) -> Result<Volt, WgError>;

    fn state() -> WaveformState;

    /// Table of an [Shape::Arbitrary] waveform, raw codes appended in order; `Open` starts a new table, `Close`
    /// completes it. The credit is the free table memory.
    #[credit]
    sink!(table: MaxLen<RefVec<'i, u8>, MAX_TABLE_CHUNK>);
}

/// What a pattern generator offers, see `PatternGenerator::capabilities`.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct PgCapabilities<'i> {
    /// Channel `n` of every configuration is `channels[n]`.
    pub channels: MaxLen<RefVec<'i, PgChannelInfo<'i>>, MAX_CHANNELS>,
    /// Highest rate per channel count, ascending by count.
    pub rates: MaxLen<RefVec<'i, RateLimit>, MAX_RATE_LIMITS>,
    /// Pattern memory in bytes.
    pub memory: u32,
    /// Accepts raw chunks.
    pub raw: bool,
    /// Accepts transitions chunks.
    pub transitions: bool,
    /// [Start::External] works.
    pub external_start: bool,
    /// [Start::AnalyzerTrigger] works.
    pub analyzer_start: bool,
}

/// One output.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct PgChannelInfo<'i> {
    /// For the user: "D0", "PB3".
    pub name: MaxLen<&'i str, MAX_NAME_LEN>,
    /// The same pin's index in the device's `ww_gpio::Bank`, if it has one.
    pub gpio_pin: Option<u8>,
}

/// What to play, see `PatternGenerator::configure`.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct PgConfig<'i> {
    /// Output channels ([PgCapabilities::channels]), in the bit order of raw words and as the channel indices of
    /// transitions chunks.
    pub channels: MaxLen<RefVec<'i, u8>, MAX_CHANNELS>,
    /// Samples per second.
    pub rate: Hertz,
    pub repeat: Repeat,
    /// Levels before the start and after the end, bit `i` for `channels[i]`.
    pub idle: u32,
    pub start: Start,
}

/// How often a pattern plays.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum Repeat {
    Once,
    Times(u32),
    Forever,
}

/// When a pattern starts after `PatternGenerator::start`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum Start {
    /// Right away.
    Now,
    /// On the external trigger input.
    External { edge: Edge },
    /// When this device's `LogicAnalyzer` triggers: a stimulus and its response on one time base.
    AnalyzerTrigger,
}

/// What `PatternGenerator::configure` actually set.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy)
)]
pub struct PgActual {
    /// Samples per second.
    pub rate: Hertz,
    /// Raw words the generator expects ([ww_logic_analyzer::RawLayout::width]).
    pub width: u8,
}

/// Where a pattern generator stands, see `PatternGenerator::status`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy),
    ww_repr = unib32
)]
pub enum PgStatus {
    /// Not configured, or stopped.
    Idle,
    /// The sink is open and `samples` are loaded.
    Loading { samples: u32 },
    /// A complete pattern is loaded.
    Ready,
    /// Playing repetition `repeat` (from 0), started at `t_start`.
    Running { repeat: u32, t_start: Second },
    /// Played to the end, started at `t_start`.
    Done { t_start: Second },
    /// Streaming couldn't keep up; output stopped at the idle levels.
    Underrun,
}

/// Errors of `PatternGenerator`. Each method lists the variants it returns.
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
pub enum PgError {
    /// Not supported by this device (an encoding, a start condition).
    Unsupported,
    /// The pins serve another function.
    InUse,
    /// The output drivers or translators have no supply.
    NoIoLevel,
    /// A safety rule of the device doesn't allow it.
    Refused,
    /// An argument is out of range (rate, channel count).
    InvalidArgument,
    /// The call needs another state (`start` with nothing loaded).
    WrongMode,
    /// No channel with this index.
    NoSuchChannel,
    /// A pattern plays.
    Busy,
}

/// What a device's DACs offer, see `WaveformGenerator::capabilities`.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct WgCapabilities<'i> {
    /// `channel[n]` is `channels[n]`.
    pub channels: MaxLen<RefVec<'i, WgChannelInfo<'i>>, MAX_CHANNELS>,
}

/// One DAC output.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct WgChannelInfo<'i> {
    /// For the user: "DAC1", "OUT A".
    pub name: MaxLen<&'i str, MAX_NAME_LEN>,
    /// Lowest output level.
    pub min: Volt,
    /// Highest output level.
    pub max: Volt,
    /// Highest frequency of the built-in shapes.
    pub max_freq: Hertz,
    /// Highest sample rate of tables.
    pub max_sample_rate: Hertz,
    /// Converter resolution.
    pub bits: u8,
    /// Table memory in samples.
    pub table_len: u32,
    /// Codes of tables to volts.
    pub code_to_volt: Linear,
    /// Table formats.
    pub formats: MaxLen<RefVec<'i, SampleFormat>, MAX_FORMATS>,
}

/// One output's waveform, see `WaveformChannel::set`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy)
)]
pub struct Waveform {
    pub shape: Shape,
    /// Repetitions per second (of the whole table for [Shape::Arbitrary]).
    pub freq: Hertz,
    /// Peak to peak.
    pub amplitude: Volt,
    pub offset: Volt,
    /// 0..=65535 = 0..360°, against the other channels started together.
    pub phase: u16,
}

/// The waveform's shape.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum Shape {
    Sine,
    /// `duty`: 0..=65535 = 0..100 %.
    Square {
        duty: u16,
    },
    Triangle,
    Sawtooth {
        rising: bool,
    },
    /// [Waveform::offset] only; frequency and amplitude are ignored.
    Dc,
    /// The `table` sink's codes in `format`, played once per period, or once and then held with `once`. Fed faster
    /// than it plays, a `once` table is a streamed DAC output. Amplitude and offset are ignored: the codes are the
    /// levels ([WgChannelInfo::code_to_volt]).
    Arbitrary {
        format: SampleFormat,
        once: bool,
    },
}

/// What `WaveformChannel::set` actually does.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy)
)]
pub struct WaveformActual {
    pub freq: Hertz,
    pub amplitude: Volt,
    pub offset: Volt,
}

/// What an output does, see `WaveformChannel::state`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone, Copy),
    ww_repr = unib32
)]
pub enum WaveformState {
    /// Holding `level`.
    Stopped { level: Volt },
    /// Set, waiting for `WaveformGenerator::start_together`.
    Held { waveform: Waveform },
    Running {
        waveform: Waveform,
        actual: WaveformActual,
    },
}

/// Errors of `WaveformGenerator`. Each method lists the variants it returns.
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
pub enum WgError {
    /// Not supported by this device (a shape, a format).
    Unsupported,
    /// The output serves another function.
    InUse,
    /// The output stage has no supply.
    NoIoLevel,
    /// A safety rule of the device doesn't allow it (a level beyond what the load may see).
    Refused,
    /// An argument is out of range (frequency, levels).
    InvalidArgument,
    /// The call needs another state (an arbitrary shape with no table).
    WrongMode,
    /// No channel with this index.
    NoSuchChannel,
}

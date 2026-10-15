//! SPI over WireWeaver: a device lets a host talk to the SPI devices on its bus, hold CS for several operations in
//! one transaction, or watch a bus without taking part.
//!
//! `Spi` is one bus: configure it, full-duplex transfers, transactions with CS held, writes, and, on adapters with
//! the hardware for it, every frame on the bus on the optional `monitor` stream. Being one trait, a device with
//! several buses is one array, `ww_impl!(spi[]: ww_spi::Spi)`, monitoring included. A host finds out whether a
//! device left `monitor` out through introspection, and what `Spi` supports through `Spi::capabilities`.
//!
//! ```rust,ignore
//! #[ww_api_root]
//! trait MyAdapter {
//!     ww_impl!(spi: ww_spi::Spi);
//! }
//! ```
//!
//! # Sizes
//!
//! Every byte buffer is bounded ([SPI_MAX], [MAX_OPS]), so a device knows the largest request and reply it has to
//! build at compile time. A device may support less, see [SpiCapabilities::max_len]. Raising a bound later is a
//! compatible change, lowering it is not.
//!
//! # Time
//!
//! `Spi::monitor` reports when a frame's CS assertion started (the time in the event head) and ended ([SpiFrame::end])
//! as [Second]s of device time: time since the device's clock started, in the prefix and number type the device
//! picks (usually a `u32` of microseconds). A fixed-width value wraps and the host unwraps it.
//!
//! # Slave mode
//!
//! `SpiConfig::role` can be [SpiRole::Slave], but data paths for slave mode (what this device sends on the next
//! select, what it received) are not defined yet; `Spi::transfer`, `Spi::transaction` and `Spi::write` assume this
//! device is the bus master.
//!
//! # Errors
//!
//! Every method returns [SpiError]: the framework's `CommonError` as [SpiError::Common], plus `BufferTooBig`. Each
//! method lists the variants it returns (`#[errors]`).
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_si::{Hertz, Second, Volt};

/// Largest data of one `Spi::transfer` or `Spi::write`, and of the reads of one `Spi::transaction` concatenated,
/// in bytes.
pub const SPI_MAX: usize = 512;

/// Most operations in one `Spi::transaction`.
pub const MAX_OPS: usize = 16;

/// Most discrete IO levels a device lists in [SpiCapabilities::io_levels].
pub const MAX_IO_LEVELS: usize = 8;

/// An SPI master: configure the bus, full-duplex transfer, transactions with CS held, and plain writes.
///
/// Several operations under one CS assertion (write a command, then read a response) are one `Spi::transaction`,
/// not several calls to `Spi::transfer`: releasing CS between them would end the frame.
#[ww_trait]
pub trait Spi {
    /// Full duplex: writes `data`, returns as many bytes as were read. CS is asserted for the call and released
    /// after, as [CsMode::Auto]; with [CsMode::Manual] the bus must already be selected (`Spi::set_cs`).
    /// `WrongState` while not configured as [SpiRole::Master], `BufferTooBig` for `data` longer than [SPI_MAX] or
    /// this device's [SpiCapabilities::max_len].
    #[errors(Common(WrongState, NoPower, Timeout), BufferTooBig)]
    fn transfer(
        data: MaxLen<RefVec<'i, u8>, SPI_MAX>,
    ) -> Result<MaxLen<RefVec<'i, u8>, SPI_MAX>, SpiError<'i>>;

    /// CS asserted for every op of `ops`, released at the end; returns the bytes of every [SpiOp::Read] and
    /// [SpiOp::Transfer], concatenated in order. [SpiOp::Delay] waits between two ops without touching the lines.
    /// Errors as `Spi::transfer`; `BufferTooBig` also when the reads together exceed [SPI_MAX], `Unsupported` for an
    /// operation of a newer version.
    #[errors(Common(Unsupported, WrongState, NoPower, Timeout), BufferTooBig)]
    fn transaction(
        ops: MaxLen<RefVec<'i, SpiOp<'i>>, MAX_OPS>,
    ) -> Result<MaxLen<RefVec<'i, u8>, SPI_MAX>, SpiError<'i>>;

    /// Writes `data`, ignoring what comes back. Errors as `Spi::transfer`.
    #[errors(Common(WrongState, NoPower, Timeout), BufferTooBig)]
    fn write(data: MaxLen<RefVec<'i, u8>, SPI_MAX>) -> Result<(), SpiError<'i>>;

    /// What this bus supports: frequency range, word sizes, master/slave, IO levels.
    fn capabilities() -> SpiCapabilities<'i>;

    /// Configures the bus and returns what was actually set: the frequency the hardware settled on (the nearest it
    /// can do, never above the one asked for) and the IO level in effect. [SpiRole::Disabled] releases the pins.
    /// `Unsupported` for [SpiRole::Slave], a `word_bits`, a [CsMode] or an IO level the adapter doesn't have,
    /// `InvalidArgument` for a `freq` outside [SpiCapabilities], `Refused` when e.g. the bus already sits at a higher
    /// voltage than `io_level`.
    #[errors(Common(Unsupported, InvalidArgument, InUse, Refused))]
    fn configure(config: SpiConfig) -> Result<SpiConfig, SpiError<'i>>;

    /// Asserts or releases CS, with [CsMode::Manual]; `WrongState` with another [CsMode].
    #[errors(Common(WrongState))]
    fn set_cs(active: bool) -> Result<(), SpiError<'i>>;

    /// Watches the bus without taking part: one [SpiFrame] per CS assertion, in bus order, decoded with `config` (the
    /// monitor doesn't drive CS or clock, it only listens). Opening starts monitoring, closing stops it. A counted
    /// stream, unit: frame; the head's time is when CS was asserted, and the host learns exactly how many frames the
    /// device dropped.
    ///
    /// Optional: an adapter without monitoring hardware leaves it out (`omit = ["spi.monitor"]`), and a host sees
    /// that in the device's introspection.
    ///
    /// # Errors
    ///
    /// `Common(InUse)`, `Common(Unsupported)` (this `word_bits`), `Common(Refused)`.
    #[optional]
    #[stream(SpiFrame<'i>, counted)]
    fn monitor(config: SpiMonitorConfig) -> Result<(), SpiError<'i>>;
}

/// Everything the bus is configured with, see `Spi::configure`: what is asked for going in, what was actually set
/// coming back.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug))]
pub struct SpiConfig {
    pub role: SpiRole,
    /// SCK frequency asked for; `Spi::configure` answers with what was actually set.
    pub freq: Hertz,
    pub mode: SpiMode,
    pub bit_order: BitOrder,
    /// Bits per word; words above 8 bits travel as 2 bytes, little-endian.
    pub word_bits: u8,
    pub cs: CsMode,
    /// Voltage of the lines on adapters with level translators ([SpiCapabilities::io_levels]); `None` leaves it as
    /// it is going in (the only choice on a device without them), and is the answer of a device without them.
    pub io_level: Option<Volt>,
}

/// Bus settings for `Spi::monitor`: everything needed to decode frames correctly, without the
/// parts that only apply to driving the bus ([SpiConfig::role], [SpiConfig::cs]).
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug))]
pub struct SpiMonitorConfig {
    pub mode: SpiMode,
    pub bit_order: BitOrder,
    pub word_bits: u8,
    /// Input threshold level, as [SpiConfig::io_level].
    pub io_level: Option<Volt>,
}

/// Bus role, set with `Spi::configure`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum SpiRole {
    /// Controller off, pins released.
    Disabled,
    Master,
    /// Data paths are not defined yet, see the crate-level docs.
    Slave,
}

/// Clock polarity and phase (CPOL/CPHA), the usual SPI mode numbering.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
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

/// Bit order of each word on the wire.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    sized,
    ww_repr = u1
)]
pub enum BitOrder {
    MsbFirst,
    LsbFirst,
}

/// How CS is driven, set with `Spi::configure`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum CsMode {
    /// Asserted for each `Spi::transfer`, `Spi::write` or `Spi::transaction`, released right after.
    Auto,
    /// Asserted and released only by `Spi::set_cs`.
    Manual,
    /// Not driven by this device at all.
    None,
}

/// One operation of a `Spi::transaction`.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug), ww_repr = unib32)]
pub enum SpiOp<'i> {
    /// Write these bytes, ignoring what comes back.
    Write(MaxLen<RefVec<'i, u8>, SPI_MAX>),
    /// Read this many bytes.
    Read(u16),
    /// Full duplex: write these bytes, keep what comes back.
    Transfer(MaxLen<RefVec<'i, u8>, SPI_MAX>),
    /// Wait, without touching the lines; CS stays asserted.
    Delay(Second),
}

/// One frame seen by `Spi::monitor`: from CS asserted to released.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug))]
pub struct SpiFrame<'i> {
    /// When CS was released; when it was asserted is the time in the event head.
    pub end: Second,
    /// Longer than [SPI_MAX] bytes, the rest was cut.
    pub truncated: bool,
    /// What the host side (controller) drove, if monitored.
    pub mosi: MaxLen<RefVec<'i, u8>, SPI_MAX>,
    /// What the device side (peripheral) drove, if monitored.
    pub miso: MaxLen<RefVec<'i, u8>, SPI_MAX>,
}

/// What this bus supports, see `Spi::capabilities`.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug))]
pub struct SpiCapabilities<'i> {
    pub min_freq: Hertz,
    pub max_freq: Hertz,
    pub min_word_bits: u8,
    pub max_word_bits: u8,
    pub master: bool,
    pub slave: bool,
    /// Discrete IO levels the adapter can set (e.g. 1.8 V, 3.3 V, 5 V); empty if it has no level control.
    pub io_levels: MaxLen<RefVec<'i, Volt>, MAX_IO_LEVELS>,
    /// Lowest IO level of an adjustable supply, if the adapter has one.
    pub io_level_min: Option<Volt>,
    /// Highest IO level of an adjustable supply, if the adapter has one.
    pub io_level_max: Option<Volt>,
    /// Largest transfer this device does, at most [SPI_MAX].
    pub max_len: u32,
}

/// Errors of `Spi`: the common ones first, then what is specific to SPI. Each method lists the variants it returns.
#[derive_shrink_wrap(
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone),
    ww_repr = unib32,
    display
)]
pub enum SpiError<'i> {
    /// {0}
    #[common]
    Common(CommonError<'i>),
    /// Longer than this device takes, see its capabilities.
    BufferTooBig,
}

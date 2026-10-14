//! SPI over WireWeaver: a device lets a host talk to the SPI devices on its bus, hold CS for several operations in
//! one transaction, or watch a bus without taking part.
//!
//! # Traits
//!
//! | Trait        | What it is for                                                        | Who implements it         |
//! |--------------|------------------------------------------------------------------------|----------------------------|
//! | `Spi`        | Configure the bus, full-duplex transfers, transactions with CS held, writes. | Every SPI master     |
//! | `SpiMonitor` | Every frame (one CS assertion) on the bus as a stream.                | Sniffers, debug adapters  |
//!
//! Monitoring is its own trait because it is its own function: a sniffer may have no master at all. A host finds
//! out what is there through introspection, and what `Spi` supports through `Spi::capabilities`.
//!
//! ```rust,ignore
//! #[ww_api_root]
//! trait MyAdapter {
//!     ww_impl!(spi: ww_spi::Spi);
//!     ww_impl!(spi_monitor: ww_spi::SpiMonitor);
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
//! `SpiMonitor` reports when a frame's CS assertion started and ended ([SpiFrame::start], [SpiFrame::end]) as
//! [Second]s of device time: time since the device's clock started, in the prefix and number type the device picks
//! (usually a `u32` of microseconds). A fixed-width value wraps and the host unwraps it.
//!
//! # Slave mode
//!
//! `SpiConfig::role` can be [SpiRole::Slave], but data paths for slave mode (what this device sends on the next
//! select, what it received) are not defined yet; `Spi::transfer`, `Spi::transaction` and `Spi::write` assume this
//! device is the bus master.
//!
//! # Errors
//!
//! Both traits return [SpiError]; each method lists the variants it returns.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_si::{Hertz, Second, Volt};

/// Largest data of one `Spi::transfer` or `Spi::write`, and of the reads of one `Spi::transaction` concatenated,
/// in bytes.
pub const SPI_MAX: usize = 512;

/// Most operations in one `Spi::transaction`.
pub const MAX_OPS: usize = 16;

/// An SPI master: configure the bus, full-duplex transfer, transactions with CS held, and plain writes.
///
/// Several operations under one CS assertion (write a command, then read a response) are one `Spi::transaction`,
/// not several calls to `Spi::transfer`: releasing CS between them would end the frame.
#[ww_trait]
pub trait Spi {
    /// Full duplex: writes `data`, returns as many bytes as were read. CS is asserted for the call and released
    /// after, as [CsMode::Auto]; with [CsMode::Manual] the bus must already be selected (`Spi::set_cs`).
    ///
    /// # Errors
    ///
    /// [SpiError::WrongMode] (not configured as [SpiRole::Master]), [SpiError::BufferTooBig] (`data` longer than
    /// [SPI_MAX] or this device's [SpiCapabilities::max_len]), [SpiError::NoIoLevel], [SpiError::Timeout].
    fn transfer(
        data: MaxLen<RefVec<'i, u8>, SPI_MAX>,
    ) -> Result<MaxLen<RefVec<'i, u8>, SPI_MAX>, SpiError>;

    /// CS asserted for every op of `ops`, released at the end; returns the bytes of every [SpiOp::Read] and
    /// [SpiOp::Transfer], concatenated in order. [SpiOp::Delay] waits between two ops without touching the lines.
    ///
    /// # Errors
    ///
    /// As `Spi::transfer`; [SpiError::InvalidArgument] (more than [MAX_OPS] operations, or the reads together
    /// exceed [SPI_MAX]).
    fn transaction(
        ops: MaxLen<RefVec<'i, SpiOp<'i>>, MAX_OPS>,
    ) -> Result<MaxLen<RefVec<'i, u8>, SPI_MAX>, SpiError>;

    /// Writes `data`, ignoring what comes back.
    ///
    /// # Errors
    ///
    /// As `Spi::transfer`.
    fn write(data: MaxLen<RefVec<'i, u8>, SPI_MAX>) -> Result<(), SpiError>;

    /// What this bus supports: frequency range, word sizes, master/slave, IO levels.
    fn capabilities() -> SpiCapabilities<'i>;

    /// Configures the bus and returns the frequency actually set (the nearest the hardware can do, never above the
    /// one asked for). [SpiRole::Disabled] releases the pins.
    ///
    /// # Errors
    ///
    /// [SpiError::InUse] (the pins or peripheral serve another function), [SpiError::Unsupported] ([SpiRole::Slave],
    /// this `word_bits`, or this [CsMode] on an adapter without it), [SpiError::InvalidArgument] (`freq` or
    /// `word_bits` outside [SpiCapabilities]), [SpiError::Refused] (a safety rule of the device).
    fn configure(config: SpiConfig) -> Result<SpiActual, SpiError>;

    /// Asserts or releases CS, with [CsMode::Manual].
    ///
    /// # Errors
    ///
    /// [SpiError::WrongMode] (CS isn't [CsMode::Manual]).
    fn set_cs(active: bool) -> Result<(), SpiError>;

    /// IO level, on adapters with level translators. [SpiError::Unsupported] if there is no level control,
    /// [SpiError::Refused] if a safety rule of the device doesn't allow the requested level.
    property!(rw reference_voltage: Volt, SpiError);
}

/// Watches an SPI bus without taking part: one [SpiFrame] per CS assertion on the `frames` stream. Configured with
/// `SpiMonitor::configure_monitor`; the stream's sideband `Open` starts, `Close` stops.
#[ww_trait]
pub trait SpiMonitor {
    /// One frame per CS assertion, in bus order.
    stream!(frames: SpiFrame<'i>);

    /// Starts or stops monitoring with the bus settings needed to decode frames (the monitor doesn't drive CS or
    /// clock, it only listens).
    ///
    /// # Errors
    ///
    /// [SpiError::InUse], [SpiError::Unsupported] (this `word_bits`), [SpiError::Refused].
    fn configure_monitor(config: SpiMonitorConfig) -> Result<(), SpiError>;
}

/// Bus settings, see `Spi::configure`.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug))]
pub struct SpiConfig {
    pub role: SpiRole,
    /// SCL frequency asked for; `Spi::configure` returns what was actually set.
    pub freq: Hertz,
    pub mode: SpiMode,
    pub bit_order: BitOrder,
    /// Bits per word; words above 8 bits travel as 2 bytes, little-endian.
    pub word_bits: u8,
    pub cs: CsMode,
}

/// What `Spi::configure` actually set.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug))]
pub struct SpiActual {
    /// The frequency the hardware settled on, never above the one asked for.
    pub freq: Hertz,
}

/// Bus settings for `SpiMonitor::configure_monitor`: everything needed to decode frames correctly, without the
/// parts that only apply to driving the bus ([SpiConfig::role], [SpiConfig::cs]).
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug))]
pub struct SpiMonitorConfig {
    pub mode: SpiMode,
    pub bit_order: BitOrder,
    pub word_bits: u8,
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

/// One frame seen by `SpiMonitor`: from CS asserted to released.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug))]
pub struct SpiFrame<'i> {
    /// When CS was asserted.
    pub start: Second,
    /// When CS was released.
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
    /// `SpiMonitor` is implemented.
    pub monitor: bool,
    /// Discrete IO levels the adapter can set (e.g. 1.8 V, 3.3 V, 5 V); empty if it has no level control.
    pub io_levels: MaxLen<RefVec<'i, Volt>, 8>,
    /// Largest transfer this device does, at most [SPI_MAX].
    pub max_len: u32,
}

/// Errors of both traits in this crate. Each method lists the variants it returns.
///
/// How errors evolve (variants from newer versions, per-method error lists, errors that carry a reason) is
/// still open for `ww_stdlib`; until then this enum stays plain.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = unib32
)]
pub enum SpiError {
    /// This adapter doesn't have it ([SpiRole::Slave], a `word_bits`, a [CsMode]).
    Unsupported,
    /// The pins or the peripheral serve another function.
    InUse,
    /// The level translators have no supply (IO level off).
    NoIoLevel,
    /// A safety rule of the device doesn't allow it.
    Refused,
    /// An argument is out of range (frequency, word size, operation count).
    InvalidArgument,
    /// The call needs another mode (e.g. `Spi::set_cs` while [CsMode] isn't `Manual`, a transfer while not a
    /// [SpiRole::Master]).
    WrongMode,
    /// Longer than this device takes, see [SpiCapabilities].
    BufferTooBig,
    Timeout,
}

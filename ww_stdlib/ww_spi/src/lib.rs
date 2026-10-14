//! SPI over WireWeaver: a device lets a host talk to the SPI devices on its bus, or watches a bus.
//!
//! | Trait        | What it is for                                                       | Who implements it        |
//! |--------------|----------------------------------------------------------------------|--------------------------|
//! | `Spi`        | Configure, full-duplex transfers, transactions with CS held, writes. | Every SPI master         |
//! | `SpiMonitor` | Every frame (one CS assertion) on the bus as a stream.               | Sniffers, debug adapters |
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_si::{Hertz, Second, Volt};

/// Largest data of one transfer, write or transaction, in bytes.
pub const SPI_MAX: usize = 512;

/// Most operations in one `Spi::transaction`.
pub const MAX_OPS: usize = 16;

/// An SPI master.
#[ww_trait]
pub trait Spi {
    /// Full duplex: writes `data` and returns as many bytes read.
    fn transfer(
        data: MaxLen<RefVec<'i, u8>, SPI_MAX>,
    ) -> Result<MaxLen<RefVec<'i, u8>, SPI_MAX>, SpiError>;

    /// CS asserted for all operations; returns the bytes of every `Read` and `Transfer`, concatenated.
    fn transaction(
        ops: MaxLen<RefVec<'i, SpiOp<'i>>, MAX_OPS>,
    ) -> Result<MaxLen<RefVec<'i, u8>, SPI_MAX>, SpiError>;

    /// Writes `data`, ignoring what comes back.
    fn write(data: MaxLen<RefVec<'i, u8>, SPI_MAX>) -> Result<(), SpiError>;

    fn capabilities() -> SpiCapabilities<'i>;

    /// Configures the bus and returns what was actually set.
    fn configure(config: SpiConfig) -> Result<SpiActual, SpiError>;

    /// Asserts or releases CS, with [CsMode::Manual].
    fn set_cs(active: bool) -> Result<(), SpiError>;

    /// IO level, on adapters with level translators.
    property!(rw reference_voltage: Volt, SpiError);
}

/// Watches an SPI bus: one [SpiFrame] per CS assertion on the `frames` stream. Configured with
/// `SpiMonitor::configure_monitor`; the stream's sideband `Open` starts, `Close` stops.
#[ww_trait]
pub trait SpiMonitor {
    stream!(frames: SpiFrame<'i>);

    fn configure_monitor(config: SpiMonitorConfig) -> Result<(), SpiError>;
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug))]
pub struct SpiConfig {
    pub role: SpiRole,
    pub freq: Hertz,
    pub mode: SpiMode,
    pub bit_order: BitOrder,
    /// Bits per word; words above 8 bits travel as 2 bytes, little-endian.
    pub word_bits: u8,
    pub cs: CsMode,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug))]
pub struct SpiActual {
    pub freq: Hertz,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug))]
pub struct SpiMonitorConfig {
    pub mode: SpiMode,
    pub bit_order: BitOrder,
    pub word_bits: u8,
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum SpiRole {
    Disabled,
    Master,
    Slave,
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    sized,
    ww_repr = u2
)]
pub enum SpiMode {
    Mode0,
    Mode1,
    Mode2,
    Mode3,
}

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

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum CsMode {
    /// Asserted for each transfer or transaction.
    Auto,
    /// `Spi::set_cs`.
    Manual,
    /// Not driven.
    None,
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug), ww_repr = unib32)]
pub enum SpiOp<'i> {
    Write(MaxLen<RefVec<'i, u8>, SPI_MAX>),
    Read(u16),
    Transfer(MaxLen<RefVec<'i, u8>, SPI_MAX>),
    Delay(Second),
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug))]
pub struct SpiFrame<'i> {
    /// CS asserted.
    pub start: Second,
    /// CS released.
    pub end: Second,
    /// Longer than [SPI_MAX] bytes, the rest was cut.
    pub truncated: bool,
    pub mosi: MaxLen<RefVec<'i, u8>, SPI_MAX>,
    pub miso: MaxLen<RefVec<'i, u8>, SPI_MAX>,
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug))]
pub struct SpiCapabilities<'i> {
    pub min_freq: Hertz,
    pub max_freq: Hertz,
    pub min_word_bits: u8,
    pub max_word_bits: u8,
    pub master: bool,
    pub slave: bool,
    pub monitor: bool,
    pub io_levels: MaxLen<RefVec<'i, Volt>, 8>,
    /// Largest transfer this device does, at most [SPI_MAX].
    pub max_len: u32,
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = unib32
)]
pub enum SpiError {
    Unsupported,
    InUse,
    NoIoLevel,
    Refused,
    InvalidArgument,
    WrongMode,
    BufferTooBig,
    Timeout,
}

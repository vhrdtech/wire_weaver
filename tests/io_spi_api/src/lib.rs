//! Test API root for `ww_spi`: every trait of the crate at one level, used by `tests/io_spi`.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait IoSpiTest {
    ww_impl!(spi: ww_spi::Spi);
}

/// Test API root for `ww_spi`'s slave mode, used by `tests/io_spi`: a device built on `ww_bus_slave::SpiSlave`,
/// driven by [SpiBusOp] instead of a real bus, with a fake clock advanced by [SpiBusOp::Advance].
#[ww_api_root]
pub trait IoSpiSlaveTest {
    ww_impl!(spi: ww_spi::Spi);

    /// Test-only: the simulated bus master issues one byte-level operation into the slave engine, in the device
    /// thread, 1:1 with `ww_bus_slave::SpiSlave`'s interrupt-side methods.
    fn spi_slave_bus(op: SpiBusOp) -> SpiBusResult;

    /// Test-only: drains the slave engine's frames onto `slave_frames` (and its drop count), so a test controls
    /// exactly when that happens instead of it running on every request.
    fn spi_slave_pump();
}

/// One byte-level operation of the simulated SPI bus master, see `IoSpiSlaveTest::spi_slave_bus`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum SpiBusOp {
    /// Advances the device's fake clock by this many microseconds, without touching the bus.
    Advance(u32),
    /// `SpiSlave::cs_asserted`.
    CsAsserted,
    /// `SpiSlave::miso`.
    Miso,
    /// `SpiSlave::mosi`.
    Mosi(u8),
    /// `SpiSlave::cs_released`.
    CsReleased,
    /// `SpiSlave::overrun`.
    Overrun,
    /// `SpiSlave::rewind`.
    Rewind,
}

/// What [SpiBusOp] did, see `IoSpiSlaveTest::spi_slave_bus`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum SpiBusResult {
    Advanced,
    Done,
    /// `SpiSlave::miso`'s return.
    Byte(u8),
}

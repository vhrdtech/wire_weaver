//! Test API root for `ww_i2c`: every trait of the crate at one level, used by `tests/io_i2c`.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait IoI2cTest {
    ww_impl!(i2c: ww_i2c::I2c);
    ww_impl!(i2c_watch: ww_i2c::I2cWatch);
    ww_impl!(i2c_monitor: ww_i2c::I2cMonitor);

    /// Test-only: runs one read cycle for every configured `I2cWatch` watch and reports results on `changes`
    /// exactly as a real device's polling loop would, instead of waiting for `period` to pass. Returns how many
    /// watches were read.
    fn tick_watches() -> u8;
}

/// Test API root for `ww_i2c`'s slave mode, used by `tests/io_i2c`: a device built on `ww_bus_slave::I2cSlave`,
/// driven by [SlaveBusOp] instead of a real bus, with a fake clock advanced by [SlaveBusOp::Advance].
#[ww_api_root]
pub trait IoI2cSlaveTest {
    ww_impl!(i2c: ww_i2c::I2c);

    /// Test-only: the simulated bus master issues one byte-level operation into the slave engine, in the device
    /// thread, 1:1 with `ww_bus_slave::I2cSlave`'s interrupt-side methods.
    fn slave_bus(op: SlaveBusOp) -> SlaveBusResult;

    /// Test-only: drains the slave engine's events onto `slave_events` (and its drop count), so a test controls
    /// exactly when that happens instead of it running on every request.
    fn slave_pump();
}

/// One byte-level operation of the simulated I2C bus master, see `IoI2cSlaveTest::slave_bus`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum SlaveBusOp {
    /// Advances the device's fake clock by this many microseconds, without touching the bus.
    Advance(u32),
    /// `I2cSlave::address`.
    Address { read: bool },
    /// `I2cSlave::restart`.
    Restart,
    /// `I2cSlave::write`.
    Write(u8),
    /// `I2cSlave::read`.
    Read,
    /// `I2cSlave::resume`.
    Resume,
    /// `I2cSlave::unsent`.
    Unsent(u32),
    /// `I2cSlave::stop`.
    Stop,
    /// `I2cSlave::bus_error`.
    BusError,
}

/// What [SlaveBusOp] did, see `IoI2cSlaveTest::slave_bus`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum SlaveBusResult {
    Advanced,
    /// `I2cSlave::address`'s return: the address was acknowledged.
    Ack(bool),
    Written,
    /// `I2cSlave::read`'s `ReadByte::Byte`.
    Byte(u8),
    /// `I2cSlave::read`'s `ReadByte::Stretch`.
    Stretch,
    /// `I2cSlave::resume`'s return.
    Resumed(Option<u8>),
    Done,
}

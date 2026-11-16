use ww_api::prelude::*;
use ww_si::Index;

use crate::{Byte, Op};

/// Errors of every trait in this crate: the common ones first, then what is specific to I2C. Each method lists the
/// variants it returns.
#[derive_shrink_wrap(
    borrowed, owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Eq, Clone),
    ww_repr = unib32,
    display
)]
pub enum I2cError {
    /// Not acknowledged: the address, or a data byte of a write (`at`), in operation `op`.
    ///
    /// `op` is 0 for `I2c::i2c_write` and a plain `I2c::i2c_read`; for an [I2cReadKind::RepeatedStart] read 0 is
    /// the write part and 1 the read part; for `I2c::i2c_transaction` the index of the [I2cOp]. `at` counts bytes on
    /// the bus within that operation as [I2cRecord::nack_at] does: `Some(0)` the address, `Some(n)` the n-th data
    /// byte (`data[n - 1]`, so `n - 1` bytes went through), `None` when the controller can't tell. A slave only refuses data on writes: in a read it is the master that doesn't
    /// acknowledge the last byte, which is no error.
    #[display("not acknowledged: operation {op}, byte {at:?} (0: the address)")]
    Nack {
        op: Index<Op, u8>,
        at: Option<Index<Byte, u16>>,
    },
    /// Arbitration lost to another master.
    Arbitration,
    /// Bus error: a start or stop where it doesn't belong.
    Bus,
    /// Packet error check (SMBus PEC) failed.
    Crc,
    /// Data came faster than the adapter could take it (slave mode).
    Overrun,
    /// Longer than this device takes, see its capabilities.
    BufferTooBig,
    /// All watch slots are taken.
    OutOfWatchSlots,
    /// No watch in this slot.
    InvalidWatchSlot,
    /// A slave held SCL low longer than the stretch timeout ([I2cMode::Master]) at once; the transfer was aborted with a stop.
    #[display("a slave held SCL low longer than the stretch timeout")]
    ClockStretch,
    /// A line is held low when the master wants to start (`scl_low`, `sda_low`: which one), so it can't: a slave
    /// stuck mid-byte, missing pull-ups or a short. The device doesn't recover on its own; `I2c::i2c_wiggle_clock`
    /// frees a stuck slave.
    #[display("bus stuck: SCL low {scl_low}, SDA low {sda_low}")]
    BusStuck { scl_low: bool, sda_low: bool },
}

ww_api::api_error!(I2cError);

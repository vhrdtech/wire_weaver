use wire_weaver::prelude::*;
use ww_si::units::{Byte, Item};
use ww_si::{Bytes, Count, Hertz, Index, Second, Volt};

use crate::{I2cAddress, I2cError};

/// Largest data of one monitor record ([I2cRecord::bytes]); a longer transaction is cut and marked
/// [I2cRecordKind::Truncated].
pub const MAX_RECORD_LEN: usize = 512;

/// Watches the bus without taking part: every transaction becomes one [I2cRecord] on the `records` stream.
///
/// The same as [I2cMode::MonitorOnly] (or the `monitor` flag of [I2cMode::Master], to watch the adapter's own
/// transfers too), for devices that only sniff and implement no `I2c`.
#[ww_trait]
pub trait I2cMonitor {
    /// One record per transaction, in bus order. Opening starts monitoring, closing stops it. `io_level` as in
    /// [I2cConfig::io_level]; the adapter's pull-ups stay as they are (the watched bus has its own). A counted
    /// stream, unit: transaction; the head's time is the start condition, and the host learns exactly how many
    /// records the device dropped. `Unsupported` for an IO level the adapter can't do.
    #[errors(Common(InUse, Unsupported, Refused, NoPower))]
    #[stream(I2cRecord<'i>, counted)]
    fn records(io_level: Option<Volt>) -> Result<(), I2cError<'i>>;
}

/// One transaction seen by `I2cMonitor`: from a start to the stop (or to the next repeated start).
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct I2cRecord<'i> {
    /// Time of the stop (or repeated start); the start condition's is the time in the event head.
    pub end: Second,
    pub addr: I2cAddress,
    /// Read (true) or write (false).
    pub read: bool,
    pub kind: I2cRecordKind,
    /// The first byte on the bus that was not acknowledged: `Some(0)` the address, `Some(n)` the n-th data byte
    /// (`bytes[n - 1]`); the same count as [I2cError::Nack]'s `at`.
    pub nack_at: Option<Index<Byte, u16>>,
    /// Data bytes, without the address.
    pub bytes: MaxLen<RefVec<'i, u8>, MAX_RECORD_LEN>,
}

/// How a monitored transaction ended.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Eq, Copy, Clone),
    ww_repr = unib32
)]
pub enum I2cRecordKind {
    /// Ended with a stop.
    Complete,
    /// Ended with a repeated start; the next record continues the transaction.
    RepeatedStart,
    /// More than [MAX_RECORD_LEN] bytes, the rest was cut.
    Truncated,
    /// A start or stop in the middle of a byte, or a line stuck.
    BusError,
}

/// Slave mode, see `I2c::slave_events`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Copy, Clone)
)]
pub struct I2cSlaveConfig {
    /// This device's address on the bus.
    pub addr: I2cAddress,
    /// As in [I2cConfig::io_level].
    pub io_level: Option<Volt>,
    /// Where the answers to the master's reads come from.
    pub answers: I2cSlaveAnswers,
}

/// Where a slave's answers come from, see [I2cSlaveConfig::answers].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Copy, Clone),
    ww_repr = unib32
)]
pub enum I2cSlaveAnswers {
    /// The register map (`I2c::slave_map_write`): the first `pointer_bytes` bytes of each master write set the
    /// pointer (big-endian, 0 for a chip without one: the pointer then only moves by reading and writing), the
    /// rest are stored from it. Reads come from the pointer. Both go up by one per byte and wrap at `len` (the
    /// first `len` bytes of the map); with `page`, writes wrap within their page of that many bytes, as in an
    /// EEPROM. With `read_only` the master's data bytes are acknowledged but not stored (they are still reported).
    Registers {
        pointer_bytes: Bytes<u8>,
        len: Bytes<u32>,
        page: Option<Bytes<u16>>,
        read_only: bool,
    },
    /// The answer queue (`I2c::slave_answers`): each master read takes the next answer; `empty` says what happens
    /// when there is none.
    Queue { empty: I2cSlaveUnderrun },
}

/// What a slave does when the master reads and no answer is queued: the host is too slow or sent nothing.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Copy, Clone),
    ww_repr = unib32
)]
pub enum I2cSlaveUnderrun {
    /// Send `fill` for every byte the master reads (`0xFF` is what a released bus reads).
    Fill(u8),
    /// Hold SCL low until an answer comes, for at most `max` per read, then send `fill` for the rest. The device
    /// sends [I2cSlaveEvent::ReadWaiting] as soon as it starts to wait. See the crate docs for the limits of
    /// stretching.
    Stretch { max: Second, fill: u8 },
    /// Don't acknowledge the address while the queue is empty, writes included, as a busy EEPROM does; a master
    /// polling for an acknowledge gets one once an answer is queued.
    Nack,
}

/// An event of `I2c::slave_events`; the start condition's time is the time in the event head.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone),
    ww_repr = unib32
)]
pub enum I2cSlaveEvent<'i> {
    /// A transaction addressed to this device ended.
    Transfer(I2cSlaveTransfer<'i>),
    /// The master reads and no answer is queued: the device holds SCL low ([I2cSlaveUnderrun::Stretch]) until an
    /// answer comes on `I2c::slave_answers` or the limit passes. Sent at once, not held back for batching. The
    /// write before it (a command, a register address) is the [I2cSlaveEvent::Transfer] just before.
    ReadWaiting,
}

/// One transaction addressed to this device: from the start (or repeated start) to the stop (or the next repeated
/// start).
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct I2cSlaveTransfer<'i> {
    /// Time of the stop (or repeated start); the start condition's is the time in the event head.
    pub end: Second,
    /// Master read (true) or write (false).
    pub read: bool,
    /// How it ended; [I2cRecordKind::Truncated] when more than [MAX_RECORD_LEN] bytes went by, also when it ended
    /// with a repeated start.
    pub kind: I2cRecordKind,
    /// [I2cSlaveAnswers::Registers]: the pointer at the first data byte (after the pointer bytes of a write).
    pub pointer: Option<Index<Byte, u32>>,
    /// The data bytes: what the master wrote (pointer bytes included), or what this device sent.
    pub bytes: MaxLen<RefVec<'i, u8>, MAX_RECORD_LEN>,
    /// Bytes of a read sent as the fill byte because no answer was there (or came too late); 0 when all came from
    /// an answer or the map.
    pub filled: Bytes<u16>,
    /// How long this device held SCL low in the transaction, waiting for an answer or for room; `None` if it
    /// didn't.
    pub stretched: Option<Second>,
}

/// What slave mode supports, see `I2c::slave_capabilities`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Copy, Clone)
)]
pub struct I2cSlaveCapabilities {
    /// Size of the register map in bytes, 0 if there is none ([I2cSlaveAnswers::Registers] unsupported).
    pub map_len: Bytes<u32>,
    /// Widest register pointer, in bytes.
    pub max_pointer_bytes: Bytes<u8>,
    /// Answers the queue holds, 0 if there is none ([I2cSlaveAnswers::Queue] unsupported).
    pub queue_len: Count<Item, u32>,
    /// Longest [I2cSlaveUnderrun::Stretch], `None` if the device can't stretch while it waits for the host.
    pub max_stretch: Option<Second>,
    /// [I2cSlaveUnderrun::Nack] works.
    pub nack_when_empty: bool,
    /// A 10-bit slave address works.
    pub ten_bit_addresses: bool,
    /// Fastest SCL the slave keeps up with.
    pub max_freq: Hertz,
}

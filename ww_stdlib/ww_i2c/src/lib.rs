//! I2C over WireWeaver: a device (a USB adapter, a dev board, an instrument) lets a host talk to the I2C devices on
//! its bus, get a stuck bus free, or watch the bus.
//!
//! # Traits
//!
//! | Trait          | What it is for                                                         | Who implements it    |
//! |----------------|------------------------------------------------------------------------|----------------------|
//! | `I2c`          | Configure the bus, write, read, write-read, transactions, scan, bus lines and recovery. | Every I2C master |
//! | `I2cWatch`     | Autonomous reads: the device repeats configured reads and reports only changes. | Adapters that offload polling |
//! | `I2cMonitor`   | Every transaction on the bus as a stream, without taking part in it.   | Sniffers, debug adapters |
//!
//! Monitoring is its own trait because it is its own function: a sniffer may have no master at all, and most
//! masters can't sniff. A host finds out what is there through introspection, and what `I2c` supports through
//! `I2c::i2c_capabilities`.
//!
//! ```rust,ignore
//! #[ww_api_root]
//! trait MyAdapter {
//!     ww_impl!(i2c: ww_i2c::I2c);
//!     ww_impl!(i2c_monitor: ww_i2c::I2cMonitor);
//! }
//! ```
//!
//! # Addresses
//!
//! [I2cAddress] is 7-bit (`0x50` for a 24C02 EEPROM, not `0xA0`) or 10-bit, as in the I2C specification and
//! embedded-hal (`SevenBitAddress`, `TenBitAddress`): `I2cAddress::seven(0x50)`, `I2cAddress::ten(0x350)`. A 7-bit
//! address takes one byte on the wire, as a plain `u8` would. An adapter without 10-bit support answers
//! [I2cError::Unsupported].
//!
//! # Sizes
//!
//! Every byte buffer is bounded ([MAX_WRITE_LEN], [MAX_READ_LEN], ...), so a device knows the largest request it
//! has to take and the largest reply it has to build at compile time. A device may support less: its limits are in
//! [I2cCapabilities]. Raising a bound later is a compatible change, lowering it is not.
//!
//! # Time
//!
//! Every transfer reports when it started and ended ([I2cTransactionTimeRange]), and every monitor record and watch
//! result when it happened (the time in the event head of their counted streams), as [Second]s of device time: time since the device's clock started, in the prefix and number type the
//! device picks (usually a `u32` of microseconds). A fixed-width value wraps (a `u32` of µs every 71.6 minutes) and
//! the host unwraps it. All timestamps of one device come from the same clock, so I2C transfers line up with the
//! device's other data.
//!
//! # Errors
//!
//! Both traits return [I2cError]; each method lists the variants it returns.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_si::{Hertz, Second, Volt};

/// Largest data of one `I2c::i2c_write`, in bytes.
pub const MAX_WRITE_LEN: usize = 512;

/// Largest data of one read (`I2c::i2c_read`, the reads of an `I2c::i2c_transaction` together), in bytes.
pub const MAX_READ_LEN: usize = 512;

/// Largest write part of a write-read ([I2cReadKind::RepeatedStart]), usually a register address of 1-2 bytes.
pub const MAX_WRITE_READ_LEN: usize = 64;

/// Most operations in one `I2c::i2c_transaction`.
pub const MAX_OPS: usize = 16;

/// Most addresses one `I2c::i2c_scan` returns: every 7-bit address.
pub const MAX_SCAN: usize = 128;

/// Largest data of one monitor record ([I2cRecord::bytes]); a longer transaction is cut and marked
/// [I2cRecordKind::Truncated].
pub const MAX_RECORD_LEN: usize = 512;

/// Most watches one `I2cWatch` device holds at once (it may hold fewer, see [I2cWatchCapabilities]).
pub const MAX_WATCHES: usize = 512;

/// Longest read of one watch, in bytes.
pub const MAX_WATCH_LEN: usize = 32;

/// Longest name in [I2cCapabilities::driver_name].
pub const MAX_NAME_LEN: usize = 32;

/// Most discrete IO levels a device lists in [I2cCapabilities::io_levels].
pub const MAX_IO_LEVELS: usize = 8;

/// An I2C master: configure the bus, write, read and scan, see the bus lines and free a stuck bus.
///
/// The most common call of a register-based sensor driver is a write-read: `I2c::i2c_read` with
/// [I2cReadKind::RepeatedStart] (register address out, data in, one transaction). Several calls in one round trip
/// are a multi-call of the client, not a method here.
#[ww_trait]
pub trait I2c {
    /// What this adapter supports: modes, frequencies, IO levels, buffer sizes, 10-bit addresses, pull-ups.
    fn i2c_capabilities() -> I2cCapabilities<'i>;

    /// Configures the bus and returns the SCL frequency actually set (the nearest the hardware can do, never above
    /// the one asked for).
    ///
    /// `io_level` sets the bus voltage on adapters with level translators; `None` leaves it as it is (and is the only
    /// choice on a device without them). [I2cMode::Disabled] releases the pins.
    ///
    /// # Errors
    ///
    /// [I2cError::UnsupportedMode], [I2cError::UnsupportedSpeed], [I2cError::UnsupportedIoLevel],
    /// [I2cError::InUse] (the pins serve another function), [I2cError::Refused] (e.g. the bus already sits at a higher
    /// voltage than `io_level`).
    fn i2c_configure(mode: I2cMode, io_level: Option<Volt>) -> Result<Hertz, I2cError>;

    /// Writes `data` to the device at `addr`: start, address, data, stop.
    ///
    /// `delay_after`: the next transfer to the same address waits this long (an EEPROM page write needs ~5 ms).
    /// Transfers to other addresses go ahead meanwhile.
    ///
    /// # Errors
    ///
    /// [I2cError::Nack] (address or data not acknowledged), [I2cError::Arbitration], [I2cError::Bus],
    /// [I2cError::Timeout] (clock stretched too long), [I2cError::WrongMode] (not a master),
    /// [I2cError::BufferTooBig] (longer than this device takes), [I2cError::Unsupported] (10-bit address on an adapter without), [I2cError::NoIoLevel].
    fn i2c_write(
        addr: I2cAddress,
        data: MaxLen<RefVec<'i, u8>, MAX_WRITE_LEN>,
        delay_after: Option<Second>,
    ) -> Result<I2cTransactionTimeRange, I2cError>;

    /// Reads `len` bytes from the device at `addr`, either plainly or after writing a few bytes with a repeated start
    /// (a register read).
    ///
    /// `delay_after` as in `I2c::i2c_write`.
    ///
    /// # Errors
    ///
    /// As `I2c::i2c_write`, and [I2cError::ZeroLengthTransfer] for `len == 0`.
    fn i2c_read(
        addr: I2cAddress,
        kind: I2cReadKind<'i>,
        len: u16,
        delay_after: Option<Second>,
    ) -> Result<I2cReadEnvelope<'i>, I2cError>;

    /// 7-bit addresses that acknowledged a one-byte read, in ascending order. The reserved addresses (`0x00..=0x07`,
    /// `0x78..=0x7F`) are skipped.
    ///
    /// # Errors
    ///
    /// [I2cError::WrongMode], [I2cError::Bus], [I2cError::Timeout], [I2cError::NoIoLevel].
    fn i2c_scan() -> Result<MaxLen<RefVec<'i, u8>, MAX_SCAN>, I2cError>;

    /// Several operations to one address with repeated starts in between and one stop at the end, like
    /// `embedded_hal::i2c::I2c::transaction`. The bytes of all [I2cOp::Read]s come back concatenated, in order.
    ///
    /// # Errors
    ///
    /// As `I2c::i2c_write`; [I2cError::BufferTooBig] when the reads together exceed [MAX_READ_LEN] or the device's
    /// limit.
    fn i2c_transaction(
        addr: I2cAddress,
        ops: MaxLen<RefVec<'i, I2cOp<'i>>, MAX_OPS>,
    ) -> Result<I2cTransactionResult<'i>, I2cError>;

    /// Levels of SCL and SDA now. Both are high on an idle bus; a line held low means a slave is stuck mid-byte,
    /// pull-ups are missing or unpowered, or something shorts it.
    fn i2c_lines_state() -> I2cLinesState;

    /// Clocks SCL up to `count` times (9 is enough for any stuck byte) until SDA is released, then sends a stop.
    /// Returns the line levels afterwards.
    ///
    /// # Errors
    ///
    /// [I2cError::WrongMode] (a transfer or the monitor uses the bus), [I2cError::Unsupported] (the controller
    /// can't drive SCL directly), [I2cError::NoIoLevel].
    fn i2c_wiggle_clock(count: u8) -> Result<I2cLinesState, I2cError>;

    /// Switches the adapter's own pull-up resistors on SCL and SDA.
    ///
    /// # Errors
    ///
    /// [I2cError::Unsupported] (no switchable pull-ups, see [I2cCapabilities::pullups]), [I2cError::Refused] (e.g.
    /// the pull-ups would feed a bus that sits at another voltage).
    fn i2c_set_pullups(enabled: bool) -> Result<(), I2cError>;

    /// Whether the adapter's pull-ups are on (false if it has none).
    fn i2c_pullups() -> bool;
}

/// Autonomous reads: the device repeats configured reads on its own and reports only what the host asked for
/// (every result, a change under a mask, or a match with a value), so the host doesn't poll over the link.
///
/// A device implementing it also implements `I2c` (it needs a master). Host transactions have priority: the device
/// runs a watch read only when no host transaction is waiting, so host calls are spliced in between autonomous
/// ones and see no more delay than one watch read. A watch to an address under a `delay_after` waits like any
/// transfer. Results go out on the `changes` stream.
#[ww_trait]
pub trait I2cWatch {
    /// Results the watches report, in the order they were read. A counted stream, unit: result; the head's time is
    /// when the read started, and the host learns exactly how many results the device dropped.
    #[counted]
    stream!(changes: I2cWatchEvent<'i>);

    /// How many watches, how fast, how long.
    fn watch_capabilities() -> I2cWatchCapabilities;

    /// Adds a watch and returns its slot, used by the other methods and in [I2cWatchEvent::slot].
    ///
    /// # Errors
    ///
    /// [I2cError::OutOfCycleSlots] (all slots taken), [I2cError::BufferTooBig] (read longer than this device
    /// watches), [I2cError::MalformedRequest] (period below the device's minimum, mask and value lengths not the
    /// read length), [I2cError::WrongMode] (not a master).
    fn watch_add(watch: I2cWatchConfig<'i>) -> Result<u32, I2cError>;

    /// Replaces a watch's configuration; its previous result is forgotten, so the next read is reported.
    ///
    /// # Errors
    ///
    /// [I2cError::InvalidCycleSlot], and as `I2cWatch::watch_add`.
    fn watch_modify(slot: u32, watch: I2cWatchConfig<'i>) -> Result<(), I2cError>;

    /// Removes a watch.
    ///
    /// # Errors
    ///
    /// [I2cError::InvalidCycleSlot].
    fn watch_remove(slot: u32) -> Result<(), I2cError>;

    /// Slots in use.
    fn watch_list() -> MaxLen<RefVec<'i, u32>, MAX_WATCHES>;
}

/// What `I2cWatch` can do.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Copy, Clone)
)]
pub struct I2cWatchCapabilities {
    /// Watches held at once, at most [MAX_WATCHES].
    pub max_watches: u32,
    /// Longest read of one watch, at most [MAX_WATCH_LEN].
    pub max_len: u16,
    /// Shortest period of one watch.
    pub min_period: Second,
}

/// One autonomous read, see `I2cWatch::watch_add`.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct I2cWatchConfig<'i> {
    pub addr: I2cAddress,
    /// Plain read, or a register read (write the register address, repeated start, read).
    pub kind: I2cReadKind<'i>,
    /// Bytes to read, at most [MAX_WATCH_LEN].
    pub len: u16,
    /// How often to read.
    pub period: Second,
    /// When to send a result.
    pub report: I2cWatchReport<'i>,
}

/// When a watch sends its result.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone),
    ww_repr = unib32
)]
pub enum I2cWatchReport<'i> {
    /// Every read.
    Always,
    /// When `data & mask` differs from the previous read's (`mask` as long as the read; all ones to see any change).
    Changed {
        mask: MaxLen<RefVec<'i, u8>, MAX_WATCH_LEN>,
    },
    /// When `data & mask == value` starts or stops being true (an alert bit set, a status reaching a state).
    Matches {
        mask: MaxLen<RefVec<'i, u8>, MAX_WATCH_LEN>,
        value: MaxLen<RefVec<'i, u8>, MAX_WATCH_LEN>,
    },
}

/// A result of a watch, on the `I2cWatch` `changes` stream; when the read started is the time in the event head. An error is reported once, when the watch starts to
/// fail, and the next successful read is reported whatever the rule.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct I2cWatchEvent<'i> {
    pub slot: u32,
    pub result: Result<MaxLen<RefVec<'i, u8>, MAX_WATCH_LEN>, I2cError>,
}

/// Watches the bus without taking part: every transaction becomes one [I2cRecord] on the `records` stream.
///
/// The same as [I2cMode::MonitorOnly] (or the `monitor` flag of [I2cMode::Master], to watch the adapter's own
/// transfers too), for devices that only sniff and implement no `I2c`.
#[ww_trait]
pub trait I2cMonitor {
    /// One record per transaction, in bus order. Opening starts monitoring, closing stops it. `io_level` as in
    /// `I2c::i2c_configure`; the adapter's pull-ups stay as they are (the watched bus has its own). A counted
    /// stream, unit: transaction; the head's time is the start condition, and the host learns exactly how many
    /// records the device dropped.
    ///
    /// # Errors
    ///
    /// [I2cError::InUse], [I2cError::UnsupportedIoLevel], [I2cError::Refused], [I2cError::NoIoLevel].
    #[stream(I2cRecord<'i>, counted)]
    fn records(io_level: Option<Volt>) -> Result<(), I2cError>;
}

/// A 7-bit or 10-bit I2C address.
///
/// Bit-packed (SW-48): a 7-bit address takes one byte on the wire (the variant bit and 7 address bits), a 10-bit
/// one 11 bits.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Eq, Copy, Clone),
    self_describing,
    ww_repr = u1
)]
pub enum I2cAddress {
    Seven(U7),
    Ten(U10),
}

impl I2cAddress {
    /// A 7-bit address, `None` above `0x7F`.
    pub fn seven(addr: u8) -> Option<Self> {
        U7::new(addr).map(I2cAddress::Seven)
    }

    /// A 10-bit address, `None` above `0x3FF`.
    pub fn ten(addr: u16) -> Option<Self> {
        U10::new(addr).map(I2cAddress::Ten)
    }
}

/// Bus mode, set with `I2c::i2c_configure`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Copy, Clone),
    ww_repr = u3
)]
pub enum I2cMode {
    /// Controller off, pins released (high-Z inputs).
    Disabled,
    /// Master at `speed` SCL; `monitor` also streams the adapter's own transfers to `I2cMonitor`.
    Master { speed: Hertz, monitor: bool },
    /// Slave at `addr`; `monitor` as above. Data paths for slave mode are not defined yet.
    Slave { addr: u8, monitor: bool },
    /// Only watch the bus, see `I2cMonitor`.
    MonitorOnly,
}

/// Plain read, or a write followed by a read with a repeated start (a register read).
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Eq, Clone),
    ww_repr = u2
)]
pub enum I2cReadKind<'i> {
    /// Start, address with read bit, `len` bytes, stop.
    Plain,
    /// Start, address with write bit, `write`, repeated start, address with read bit, `len` bytes, stop.
    RepeatedStart {
        write: MaxLen<RefVec<'i, u8>, MAX_WRITE_READ_LEN>,
    },
}

/// One operation of an `I2c::i2c_transaction`.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Eq, Clone),
    ww_repr = u2
)]
pub enum I2cOp<'i> {
    /// Write these bytes.
    Write(MaxLen<RefVec<'i, u8>, MAX_WRITE_LEN>),
    /// Read this many bytes.
    Read(u16),
}

/// When a transfer started (start condition) and ended (stop condition), in device time.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Copy, Clone)
)]
pub struct I2cTransactionTimeRange {
    pub start: Second,
    pub end: Second,
}

/// Result of `I2c::i2c_read`.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct I2cReadEnvelope<'i> {
    pub duration: I2cTransactionTimeRange,
    pub data: MaxLen<RefVec<'i, u8>, MAX_READ_LEN>,
}

/// Result of `I2c::i2c_transaction`.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct I2cTransactionResult<'i> {
    pub duration: I2cTransactionTimeRange,
    /// The bytes of every [I2cOp::Read], concatenated in order.
    pub read: MaxLen<RefVec<'i, u8>, MAX_READ_LEN>,
}

/// Levels of the bus lines, see `I2c::i2c_lines_state`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Copy, Clone)
)]
pub struct I2cLinesState {
    pub is_scl_high: bool,
    pub is_sda_high: bool,
    /// Measured SCL voltage, on adapters that can measure it.
    pub scl: Option<Volt>,
    /// Measured SDA voltage, on adapters that can measure it.
    pub sda: Option<Volt>,
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
    /// Index of the first byte that was not acknowledged; `Some(0)` = the address was not.
    pub nack_at: Option<u16>,
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

/// What the adapter supports, see `I2c::i2c_capabilities`.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct I2cCapabilities<'i> {
    /// Discrete IO levels the adapter can set (e.g. 1.8 V, 3.3 V, 5 V); empty if it has no level control.
    pub io_levels: MaxLen<RefVec<'i, Volt>, MAX_IO_LEVELS>,
    /// Lowest IO level of an adjustable supply, if the adapter has one.
    pub io_level_min: Option<Volt>,
    /// Highest IO level of an adjustable supply, if the adapter has one.
    pub io_level_max: Option<Volt>,
    /// Largest write this device takes, at most [MAX_WRITE_LEN].
    pub max_write_len: u32,
    /// Largest read this device does, at most [MAX_READ_LEN].
    pub max_read_len: u32,
    /// Lowest SCL frequency.
    pub min_freq: Hertz,
    /// Highest SCL frequency.
    pub max_freq: Hertz,
    /// Driver or controller, for the user: "STM32 I2C1", "RP2350 I2C0 + PIO monitor".
    pub driver_name: MaxLen<&'i str, MAX_NAME_LEN>,
    pub master_supported: bool,
    pub slave_supported: bool,
    pub monitor_supported: bool,
    /// 10-bit addresses work.
    pub ten_bit_addresses: bool,
    /// `I2c::i2c_set_pullups` works.
    pub pullups: bool,
    /// `I2c::i2c_wiggle_clock` works.
    pub bus_recovery: bool,
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
pub enum I2cError {
    /// The mode isn't supported by this adapter.
    UnsupportedMode,
    /// The call needs another mode (e.g. a write while not a master).
    WrongMode,
    /// The frequency is out of the adapter's range.
    UnsupportedSpeed,
    /// All `I2cWatch` slots are taken.
    OutOfCycleSlots,
    /// No `I2cWatch` with this slot.
    InvalidCycleSlot,
    /// An argument is out of range.
    MalformedRequest,
    /// Longer than this device takes, see [I2cCapabilities].
    BufferTooBig,
    /// The IO level can't be set on this adapter.
    UnsupportedIoLevel,
    /// Bus error: a start or stop where it doesn't belong.
    Bus,
    /// Arbitration lost to another master.
    Arbitration,
    /// The address or a data byte was not acknowledged.
    Nack,
    /// A slave stretched the clock too long, or the bus stayed busy.
    Timeout,
    /// Packet error check (SMBus PEC) failed.
    Crc,
    /// Data came faster than the adapter could take it (slave mode).
    Overrun,
    /// A read or write of zero bytes.
    ZeroLengthTransfer,
    /// The pins or the controller serve another function.
    InUse,
    /// The level translators have no supply (IO level off).
    NoIoLevel,
    /// A safety rule of the device doesn't allow it.
    Refused,
    /// This adapter doesn't have it (10-bit addresses, switchable pull-ups, bus recovery).
    Unsupported,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Checked against the real encoding (AGENTS.md): a 7-bit address is the variant bit (`self_describing`,
    /// `ww_repr = u1`) plus 7 address bits, 8 in total, so it costs no more than a plain `u8` would.
    #[test]
    fn seven_bit_address_is_one_byte_on_the_wire() {
        let addr = I2cAddress::seven(0x50).unwrap();
        let mut buf = [0u8; 4];
        let bytes = addr.to_ww_bytes(&mut buf).unwrap();
        assert_eq!(bytes.len(), 1);
        assert_eq!(I2cAddress::from_ww_bytes(bytes).unwrap(), addr);
    }

    /// A 10-bit address is the variant bit plus 10 address bits, 11 in total, rounded up to 2 bytes.
    #[test]
    fn ten_bit_address_is_two_bytes_on_the_wire() {
        let addr = I2cAddress::ten(0x350).unwrap();
        let mut buf = [0u8; 4];
        let bytes = addr.to_ww_bytes(&mut buf).unwrap();
        assert_eq!(bytes.len(), 2);
        assert_eq!(I2cAddress::from_ww_bytes(bytes).unwrap(), addr);
    }

    #[test]
    fn address_constructors_reject_out_of_range_values() {
        assert!(I2cAddress::seven(0x80).is_none());
        assert!(I2cAddress::ten(0x400).is_none());
    }
}

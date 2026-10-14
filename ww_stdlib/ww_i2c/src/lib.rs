//! I2C over WireWeaver: a device (a USB adapter, a dev board, an instrument) lets a host talk to the I2C devices on
//! its bus, watch the bus, or get a stuck bus free.
//!
//! # Traits
//!
//! | Trait          | What it is for                                                       | Who implements it            |
//! |----------------|----------------------------------------------------------------------|------------------------------|
//! | `I2c`          | Configure the bus, write, read, write-read, transactions, scan.      | Every I2C master             |
//! | `I2cBus`       | Line levels, clocking a stuck slave free, switchable pull-ups.       | Adapters with that hardware  |
//! | `I2cMonitor`   | Every transaction on the bus as a stream, without taking part in it. | Sniffers, debug adapters     |
//!
//! A device puts only the traits it supports into its API tree, so a plain MCU bridge implements `I2c` and pays
//! nothing for the others. A host finds out what is there through introspection, and what each trait supports
//! through `I2c::i2c_capabilities`.
//!
//! ```rust,ignore
//! #[ww_api_root]
//! trait MyAdapter {
//!     ww_impl!(i2c: ww_i2c::I2c);
//!     ww_impl!(i2c_bus: ww_i2c::I2cBus);
//!     ww_impl!(i2c_monitor: ww_i2c::I2cMonitor);
//! }
//! ```
//!
//! # Addresses
//!
//! Addresses are 7-bit, in a `u8` (`0x50` for a 24C02 EEPROM, not `0xA0`); a value above `0x7F` is answered with
//! [I2cError::MalformedRequest]. 10-bit addressing is not supported yet.
//!
//! # Sizes
//!
//! Every byte buffer is bounded ([MAX_WRITE_LEN], [MAX_READ_LEN], ...), so a device knows the largest request it
//! has to take and the largest reply it has to build at compile time. A device may support less: its limits are in
//! [I2cCapabilities]. Raising a bound later is a compatible change, lowering it is not.
//!
//! # Time
//!
//! Every transfer reports when it started and ended ([I2cTransactionTimeRange]), and every monitor record when it
//! happened, as [Second]s of device time: time since the device's clock started, in the prefix and number type the
//! device picks (usually a `u32` of microseconds). A fixed-width value wraps (a `u32` of µs every 71.6 minutes) and
//! the host unwraps it. All timestamps of one device come from the same clock, so I2C transfers line up with the
//! device's other streams. A device without a usable clock reports zero and says so in
//! [I2cCapabilities::timestamps].
//!
//! # Errors
//!
//! All traits return [I2cError]; each method lists the variants it returns. The `InUse`, `NoIoLevel` and `Refused`
//! variants mean the same in every IO crate of `ww_stdlib`, so generic tools show them alike.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_si::{Second, Volt};

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

/// Longest name in [I2cCapabilities::driver_name] and in [I2cError::InUse].
pub const MAX_NAME_LEN: usize = 32;

/// Longest reason in [I2cError::Refused].
pub const MAX_REASON_LEN: usize = 64;

/// Most discrete IO levels a device lists in [I2cCapabilities::io_levels].
pub const MAX_IO_LEVELS: usize = 8;

/// An I2C master: configure the bus, then write, read and scan.
///
/// The most common call of a register-based sensor driver is a write-read: `I2c::i2c_read` with
/// [I2cReadKind::RepeatedStart] (register address out, data in, one transaction). Several calls in one round trip
/// are a multi-call of the client, not a method here.
#[ww_trait]
pub trait I2c {
    /// What this adapter supports: modes, frequencies, IO levels, buffer sizes, timestamps.
    fn i2c_capabilities() -> I2cCapabilities<'i>;

    /// Configures the bus and returns the SCL frequency actually set, in Hz (the nearest the hardware can do, never
    /// above the one asked for).
    ///
    /// `io_level` sets the bus voltage on adapters with level translators; `None` leaves it as it is (and is the only
    /// choice on a device without them). [I2cMode::Disabled] releases the pins.
    ///
    /// # Errors
    ///
    /// [I2cError::UnsupportedMode], [I2cError::UnsupportedSpeed], [I2cError::UnsupportedIoLevel],
    /// [I2cError::InUse] (the pins serve another function), [I2cError::Refused] (e.g. the bus already sits at a higher
    /// voltage than `io_level`).
    fn i2c_configure(mode: I2cMode, io_level: Option<Volt>) -> Result<u32, I2cError<'i>>;

    /// Writes `data` to the device at `addr`: start, address, data, stop.
    ///
    /// `delay_after`: the next transfer to the same address waits this long (an EEPROM page write needs ~5 ms).
    /// Transfers to other addresses go ahead meanwhile.
    ///
    /// # Errors
    ///
    /// [I2cError::Nack] (address or data not acknowledged), [I2cError::Arbitration], [I2cError::Bus],
    /// [I2cError::Timeout] (clock stretched too long), [I2cError::WrongMode] (not a master),
    /// [I2cError::BufferTooBig] (longer than this device takes), [I2cError::NoIoLevel].
    fn i2c_write(
        addr: u8,
        data: MaxLen<RefVec<'i, u8>, MAX_WRITE_LEN>,
        delay_after: Option<Second>,
    ) -> Result<I2cTransactionTimeRange, I2cError<'i>>;

    /// Reads `len` bytes from the device at `addr`, either plainly or after writing a few bytes with a repeated start
    /// (a register read).
    ///
    /// `delay_after` as in `I2c::i2c_write`.
    ///
    /// # Errors
    ///
    /// As `I2c::i2c_write`, and [I2cError::ZeroLengthTransfer] for `len == 0`.
    fn i2c_read(
        addr: u8,
        kind: I2cReadKind<'i>,
        len: u16,
        delay_after: Option<Second>,
    ) -> Result<I2cReadEnvelope<'i>, I2cError>;

    /// Addresses that acknowledged a one-byte read, in ascending order. The reserved addresses (`0x00..=0x07`,
    /// `0x78..=0x7F`) are skipped.
    ///
    /// # Errors
    ///
    /// [I2cError::WrongMode], [I2cError::Bus], [I2cError::Timeout], [I2cError::NoIoLevel].
    fn i2c_scan() -> Result<MaxLen<RefVec<'i, u8>, MAX_SCAN>, I2cError<'i>>;

    /// Several operations to one address with repeated starts in between and one stop at the end, like
    /// `embedded_hal::i2c::I2c::transaction`. The bytes of all [I2cOp::Read]s come back concatenated, in order.
    ///
    /// # Errors
    ///
    /// As `I2c::i2c_write`; [I2cError::BufferTooBig] when the reads together exceed [MAX_READ_LEN] or the device's
    /// limit.
    fn i2c_transaction(
        addr: u8,
        ops: MaxLen<RefVec<'i, I2cOp<'i>>, MAX_OPS>,
    ) -> Result<I2cTransactionResult<'i>, I2cError<'i>>;
}

/// Direct control of the bus lines, for adapters that have it: see the levels, free a stuck bus, switch pull-ups.
#[ww_trait]
pub trait I2cBus {
    /// Levels of SCL and SDA now. Both are high on an idle bus; a line held low means a slave is stuck mid-byte,
    /// pull-ups are missing or unpowered, or something shorts it.
    fn lines_state() -> I2cLinesState;

    /// Clocks SCL up to `count` times (9 is enough for any stuck byte) until SDA is released, then sends a stop.
    /// Returns the line levels afterwards.
    ///
    /// # Errors
    ///
    /// [I2cError::WrongMode] (the bus is in use by a transfer or the monitor), [I2cError::Unsupported],
    /// [I2cError::NoIoLevel].
    fn wiggle_clock(count: u8) -> Result<I2cLinesState, I2cError<'i>>;

    /// Switches the adapter's own pull-up resistors on SCL and SDA.
    ///
    /// # Errors
    ///
    /// [I2cError::Unsupported] (no switchable pull-ups), [I2cError::Refused] (e.g. the pull-ups would feed a bus that
    /// sits at another voltage).
    fn set_pullups(enabled: bool) -> Result<(), I2cError<'i>>;

    /// Whether the adapter's pull-ups are on; `None` if it has none.
    fn pullups() -> Option<bool>;
}

/// Watches the bus without taking part: every transaction becomes one [I2cRecord] on the `records` stream.
///
/// The same as [I2cMode::MonitorOnly] (or the `monitor` flag of [I2cMode::Master] to watch the adapter's own
/// transfers too), for devices that only sniff and implement no `I2c`.
#[ww_trait]
pub trait I2cMonitor {
    /// One record per transaction, in bus order. `seq` counts records.
    stream!(records: I2cRecord<'i>);

    /// Starts or stops monitoring. `io_level` as in `I2c::i2c_configure`; the adapter's pull-ups stay as they are
    /// (the watched bus has its own).
    ///
    /// # Errors
    ///
    /// [I2cError::InUse], [I2cError::UnsupportedIoLevel], [I2cError::Refused], [I2cError::NoIoLevel].
    fn monitor(enabled: bool, io_level: Option<Volt>) -> Result<(), I2cError<'i>>;
}

/// Bus mode, set with `I2c::i2c_configure`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Eq, Copy, Clone),
    self_describing,
    ww_repr = u3
)]
pub enum I2cMode {
    /// Controller off, pins released (high-Z inputs).
    Disabled,
    /// Master at `speed_hz` SCL; `monitor` also streams the adapter's own transfers to `I2cMonitor`.
    Master { speed_hz: u32, monitor: bool },
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

/// Levels of the bus lines, see `I2cBus::lines_state`.
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
    /// Number of this record since monitoring started, wrapping; counts lost records too.
    pub seq: u32,
    /// Records dropped right before this one (the host didn't keep up), 0 normally.
    pub lost: UNib32,
    /// Time of the start condition.
    pub t: Second,
    /// Time of the stop (or repeated start).
    pub t_end: Second,
    /// 7-bit address.
    pub addr: u8,
    /// Read (true) or write (false).
    pub read: bool,
    pub kind: I2cRecordKind,
    /// Index of the first byte that was not acknowledged; `Some(0)` = the address was not.
    pub nack_at: Option<u16>,
    /// Data bytes, without the address byte.
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
    /// A kind from a newer version of this API.
    #[future_version]
    Unknown(u32),
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
    pub min_frequency_hz: u32,
    pub max_frequency_hz: u32,
    /// Driver or controller, for the user: "STM32 I2C1", "RP2350 I2C0 + PIO monitor".
    pub driver_name: MaxLen<&'i str, MAX_NAME_LEN>,
    pub master_supported: bool,
    pub slave_supported: bool,
    pub monitor_supported: bool,
    /// Whether timestamps are real; without a clock they are all zero.
    pub timestamps: bool,
}

/// Errors of every trait in this crate. Each method lists the variants it returns.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Eq, Clone),
    ww_repr = unib32
)]
pub enum I2cError<'i> {
    /// The mode isn't supported by this adapter.
    UnsupportedMode,
    /// The call needs another mode (e.g. a write while not a master).
    WrongMode,
    /// The frequency is out of the adapter's range.
    UnsupportedSpeed,
    /// Reserved for cycle reads (later).
    OutOfCycleSlots,
    /// Reserved for cycle reads (later).
    InvalidCycleSlot,
    /// An argument is out of range (e.g. an address above `0x7F`).
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
    /// The pins or the controller serve another function, named in `by` ("SPI1", "UART monitor").
    InUse { by: MaxLen<&'i str, MAX_NAME_LEN> },
    /// The level translators have no supply (IO level off).
    NoIoLevel,
    /// A safety rule of the device doesn't allow it; `reason` says which.
    Refused {
        reason: MaxLen<&'i str, MAX_REASON_LEN>,
    },
    /// This adapter doesn't have it (e.g. no switchable pull-ups).
    Unsupported,
    /// An error from a newer version of this API.
    #[future_version]
    Unknown(u32),
}

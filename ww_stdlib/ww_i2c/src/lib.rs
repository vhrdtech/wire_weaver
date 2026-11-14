//! I2C over WireWeaver: a device (a USB adapter, a dev board, an instrument) lets a host talk to the I2C devices on
//! its bus, get a stuck bus free, or watch the bus.
//!
//! # Traits
//!
//! | Trait          | What it is for                                                         | Who implements it    |
//! |----------------|------------------------------------------------------------------------|----------------------|
//! | `I2c`          | Configure the bus, write, read, write-read, transactions, scan, bus lines and recovery; optionally be a slave. | Every I2C master |
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
//! `CommonError::Unsupported`.
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
//! # Slave mode
//!
//! A device with the `slave` group of `I2c` (`#[optional(slave)]`, left out with `omit = ["i2c.slave"]`) answers
//! another master on its bus at an address of its own, as a chip it stands in for. Opening `I2c::slave_events`
//! with an [I2cSlaveConfig] starts it, closing the stream stops it (the address is not acknowledged any more). What
//! the device answers when the master reads comes from one of two sources ([I2cSlaveAnswers]):
//!
//! - **Register map**: a byte array on the device that the host fills and reads back with `I2c::slave_map_write` /
//!   `I2c::slave_map_read`, before and while the slave runs. The master's first bytes of a write set a pointer, the
//!   rest are stored from it; reads come from the pointer; both go up by one per byte. The device answers at bus
//!   speed, the host is never in the loop. For EEPROMs, sensors, PMICs and any register-based chip.
//! - **Answer queue**: answers the host sends on the `I2c::slave_answers` credit sink, one per read transaction.
//!   For command-response protocols where the host computes each answer. When the master reads and no answer is
//!   queued ([I2cSlaveUnderrun]), the device sends a fill byte, holds SCL low (clock stretching) until the host
//!   answers or a time limit passes, or does not acknowledge its address while the queue is empty.
//!
//! Every transaction addressed to the device comes back on `I2c::slave_events` as an [I2cSlaveTransfer] with the
//! bytes written or sent, so the host sees what the master did either way.
//!
//! ## Clock stretching and its limits
//!
//! A host answer takes a round trip over the link: on USB full speed a few milliseconds, longer on a busy host
//! (measure it on the device at hand: [I2cSlaveTransfer::stretched]). The I2C specification puts no limit on
//! stretching, but SMBus devices give up after 25 ms (`tTIMEOUT`), many masters have a timeout of their own, and
//! some don't support stretching at all (the Raspberry Pi's BCM2835 I2C controller corrupts the transfer). So
//! [I2cSlaveUnderrun::Stretch] has a limit, after which the device sends the fill byte and reports the late answer
//! ([I2cSlaveTransfer::filled]); a late answer then goes to the next read. Prefer the register map whenever the
//! answer can be prepared ahead.
//!
//! # Errors
//!
//! Every trait returns [I2cError]: the framework's `CommonError` (in use, no power, refused, ...) as
//! [I2cError::Common], plus what is specific to I2C (`Nack`, `Arbitration`, ...). Each method lists the variants
//! it returns (`#[errors]`).
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_si::units::{Byte, Cycle, Item};
pub use ww_si::{Bytes, Count, Hertz, Index, Range, Second, Volt};

pub mod error;
pub use error::I2cError;
pub mod monitor;
pub use monitor::{
    I2cMonitor, I2cRecord, I2cRecordKind, I2cSlaveAnswers, I2cSlaveCapabilities, I2cSlaveConfig,
    I2cSlaveEvent, I2cSlaveTransfer, I2cSlaveUnderrun, MAX_RECORD_LEN,
};
pub mod watch;
pub use watch::{
    I2cMatchByte, I2cWatch, I2cWatchCapabilities, I2cWatchConfig, I2cWatchEvent, I2cWatchReport,
    MAX_WATCH_LEN, MAX_WATCHES, WatchSlot,
};

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

/// Longest name in [I2cCapabilities::driver_name].
pub const MAX_NAME_LEN: usize = 32;

/// Most discrete IO levels a device lists in [I2cCapabilities::io_levels].
pub const MAX_IO_LEVELS: usize = 8;

/// Most discrete SCL frequencies a device lists in [I2cCapabilities::speeds].
pub const MAX_SPEEDS: usize = 8;

ww_si::unit! {
    /// An operation's position in an `I2c::i2c_transaction`: `0` for `I2c::i2c_write` and a plain
    /// `I2c::i2c_read`, `0`/`1` for the write/read part of a repeated-start read, the index into `ops` for
    /// `i2c_transaction`. See [I2cError::Nack]'s `op`.
    pub struct Op = "op" (long "operation" plural "operations"), base;
    /// A watch an `I2cWatch` device holds, see [I2cWatchCapabilities::max_watches].
    pub struct Watch = "watch" (long "watch" plural "watches"), base;
}

/// An I2C master: configure the bus, write, read and scan, see the bus lines and free a stuck bus.
///
/// The most common call of a register-based sensor driver is a write-read: `I2c::i2c_read` with
/// [I2cReadKind::RepeatedStart] (register address out, data in, one transaction). Several calls in one round trip
/// are a multi-call of the client, not a method here.
///
/// Every transfer starts with a start condition and the address, and ends with a stop; inside one call each
/// operation starts with a repeated start and the address again (see `I2c::i2c_transaction`). A device never frees
/// a stuck bus on its own: it answers [I2cError::BusStuck], and the host decides to call `I2c::i2c_wiggle_clock`.
#[ww_trait]
#[claim(exclusive)]
pub trait I2c {
    /// What this adapter supports: modes, frequencies, IO levels, buffer sizes, 10-bit addresses, pull-ups.
    fn i2c_capabilities() -> I2cCapabilities<'i>;

    /// Configures the bus and returns what was actually set: the SCL frequency (the highest the hardware can do that
    /// is not above the request, computed from its real timing), the IO level and the clock stretch timeout in
    /// effect. [I2cMode::Disabled] releases the pins.
    /// A mode or IO level this adapter doesn't have is `Unsupported`, a frequency outside its range an
    /// `InvalidArgument`; `Refused` when e.g. the bus already sits at a higher voltage than `io_level`.
    #[errors(Common(Unsupported, InvalidArgument, InUse, Refused))]
    fn i2c_configure(config: I2cConfig) -> Result<I2cConfig, I2cError<'i>>;

    /// Writes `data` to the device at `addr`: start, address, data, stop.
    ///
    /// `delay_after`: the next transfer to the same address waits this long (an EEPROM page write needs ~5 ms).
    /// Transfers to other addresses go ahead meanwhile. Empty `data` is an address-only write (a probe, SMBus Quick
    /// Command) where [I2cCapabilities::zero_length_write] says so.
    ///
    /// A 10-bit address or an empty write on an adapter without them is `Unsupported`, a call while not a master
    /// `WrongState`. `Nack` says what was not acknowledged; a slave holding SCL low too long is `ClockStretch`, a
    /// line held low before the start `BusStuck`, a call that doesn't finish within the device's own deadline
    /// (waiting for another master to free the bus) `Timeout`.
    #[errors(
        Common(Unsupported, WrongState, NoPower, Timeout),
        Nack,
        Arbitration,
        Bus,
        BufferTooBig,
        ClockStretch,
        BusStuck
    )]
    fn i2c_write(
        addr: I2cAddress,
        data: MaxLen<RefVec<'i, u8>, MAX_WRITE_LEN>,
        delay_after: Option<Second>,
    ) -> Result<I2cTransactionTimeRange, I2cError<'i>>;

    /// Reads `len` bytes from the device at `addr`, either plainly or after writing a few bytes with a repeated start
    /// (a register read).
    ///
    /// `delay_after` as in `I2c::i2c_write`. Errors as there, and `InvalidArgument` for `len == 0`. A
    /// [I2cReadKind::RepeatedStart] read is the transaction `[Write(write), Read(len)]`, so a `Nack` in its read part
    /// has `op: 1`.
    #[errors(
        Common(Unsupported, InvalidArgument, WrongState, NoPower, Timeout),
        Nack,
        Arbitration,
        Bus,
        BufferTooBig,
        ClockStretch,
        BusStuck
    )]
    fn i2c_read(
        addr: I2cAddress,
        kind: I2cReadKind<'i>,
        len: Bytes<u16>,
        delay_after: Option<Second>,
    ) -> Result<I2cReadEnvelope<'i>, I2cError<'i>>;

    /// 7-bit addresses that acknowledged a one-byte read, in ascending order. The reserved addresses (`0x00..=0x07`,
    /// `0x78..=0x7F`) are skipped. An address that doesn't answer is no error; a stuck bus stops the scan.
    #[errors(
        Common(WrongState, NoPower, Timeout),
        Bus,
        Arbitration,
        ClockStretch,
        BusStuck
    )]
    fn i2c_scan() -> Result<MaxLen<RefVec<'i, I2cAddress>, MAX_SCAN>, I2cError<'i>>;

    /// Several operations to one address in one transaction: a start, then for each operation the address with its
    /// direction bit and the operation's bytes, a repeated start before every operation after the first, and one
    /// stop at the end, as Linux's `I2C_RDWR`. The bytes of all [I2cOp::Read]s come back concatenated, in order.
    ///
    /// Two writes in a row are two address phases, not one longer write: this is not the merging of
    /// `embedded_hal::i2c::I2c::transaction`, where adjacent operations of the same direction run without a repeated
    /// start. A host adapter for embedded-hal merges them before the call (writes concatenated into one
    /// [I2cOp::Write], reads into one [I2cOp::Read] of the summed length, its reads split again from the result).
    ///
    /// Errors as `I2c::i2c_write`, `Nack::op` being the index into `ops`; `BufferTooBig` when the reads together
    /// exceed [MAX_READ_LEN] or the device's limit, or for more than [I2cCapabilities::max_ops] operations;
    /// `InvalidArgument` for no operations or an `I2cOp::Read(0)`.
    #[errors(
        Common(Unsupported, InvalidArgument, WrongState, NoPower, Timeout),
        Nack,
        Arbitration,
        Bus,
        BufferTooBig,
        ClockStretch,
        BusStuck
    )]
    fn i2c_transaction(
        addr: I2cAddress,
        ops: MaxLen<RefVec<'i, I2cOp<'i>>, MAX_OPS>,
    ) -> Result<I2cTransactionResult<'i>, I2cError<'i>>;

    /// Levels of SCL and SDA now. Both are high on an idle bus; a line held low means a slave is stuck mid-byte,
    /// pull-ups are missing or unpowered, or something shorts it.
    fn i2c_lines_state() -> I2cLinesState;

    /// Bus recovery: clocks SCL up to `count` times (9 is enough for any stuck byte), at the configured SCL frequency
    /// but at most 100 kHz, stopping early once SDA is released, then sends a stop. Returns the line levels
    /// afterwards: both high when the bus is free again. Works while the bus is disabled or a master between
    /// transfers; `WrongState` while a transfer or the monitor uses the bus, `Unsupported` when the controller can't
    /// drive SCL directly, `NoPower` when the bus has no level to drive (level translators without an IO level, a
    /// bus that is off while disabled). The only way a device frees the bus: it never does so silently before a
    /// transfer.
    #[errors(Common(Unsupported, WrongState, NoPower))]
    fn i2c_wiggle_clock(count: Count<Cycle, u8>) -> Result<I2cLinesState, I2cError<'i>>;

    /// Switches the adapter's own pull-up resistors on SCL and SDA. `Unsupported` without switchable pull-ups (see
    /// [I2cCapabilities::pullups]), `Refused` when e.g. the pull-ups would feed a bus that sits at another voltage.
    #[errors(Common(Unsupported, Refused))]
    fn i2c_set_pullups(enabled: bool) -> Result<(), I2cError<'i>>;

    /// Whether the adapter's pull-ups are on (false if it has none).
    fn i2c_pullups() -> bool;

    /// What slave mode supports: register map size, answer queue depth, clock stretching, address NACK.
    #[optional(slave)]
    fn slave_capabilities() -> I2cSlaveCapabilities;

    /// Slave mode: opening the stream makes this device a slave at [I2cSlaveConfig::addr] answering from
    /// [I2cSlaveConfig::answers], closing it stops that (the address is not acknowledged any more). One event per
    /// transaction addressed to this device, in bus order, and [I2cSlaveEvent::ReadWaiting] when the master waits for
    /// an answer the host has not sent yet. A counted stream, unit: event; the head's time is the start condition,
    /// and the host learns exactly how many events the device dropped.
    ///
    /// `InUse` while the bus is a master or monitor (or another slave), `Unsupported` for a source, underrun
    /// policy, pointer width or 10-bit address the device can't do ([I2cSlaveCapabilities]), `InvalidArgument` for
    /// a stretch limit above [I2cSlaveCapabilities::max_stretch], `BufferTooBig` for a register map larger than
    /// [I2cSlaveCapabilities::map_len].
    #[optional(slave)]
    #[errors(
        Common(InUse, Unsupported, InvalidArgument, Refused, NoPower),
        BufferTooBig
    )]
    #[stream(I2cSlaveEvent<'i>, counted)]
    fn slave_events(config: I2cSlaveConfig) -> Result<(), I2cError<'i>>;

    /// Writes `data` into the register map from `offset`, while the slave runs or before it starts (the map keeps
    /// its contents between runs). Takes effect between two master transactions, never inside one, so a master
    /// never reads half of an update. `BufferTooBig` past [I2cSlaveCapabilities::map_len].
    #[optional(slave)]
    #[errors(BufferTooBig)]
    fn slave_map_write(
        offset: Index<Byte, u32>,
        data: MaxLen<RefVec<'i, u8>, MAX_WRITE_LEN>,
    ) -> Result<(), I2cError<'i>>;

    /// Reads `len` bytes of the register map from `offset` as the master sees it: what the master wrote, or what
    /// the host put there. A `I2c::slave_map_write` still waiting for the master's transaction to end shows once it
    /// is applied.
    /// `BufferTooBig` past [I2cSlaveCapabilities::map_len] or for `len` above [MAX_READ_LEN].
    #[optional(slave)]
    #[errors(BufferTooBig)]
    fn slave_map_read(
        offset: Index<Byte, u32>,
        len: Bytes<u16>,
    ) -> Result<MaxLen<RefVec<'i, u8>, MAX_READ_LEN>, I2cError<'i>>;

    /// Answers for [I2cSlaveAnswers::Queue], one item per read transaction of the master, in order. A master that
    /// reads fewer bytes than the answer has leaves the rest unsent (dropped at the stop or repeated start), one that
    /// reads more gets the fill byte for the rest. Credit-based (API-28): the host never sends more than the device
    /// has room for ([I2cSlaveCapabilities::queue_len] answers).
    #[optional(slave)]
    #[credit]
    sink!(slave_answers: MaxLen<RefVec<'i, u8>, MAX_READ_LEN>);

    /// Drops every queued answer (after the master gave up on a request, say) and returns how many there were.
    #[optional(slave)]
    fn slave_clear_answers() -> Count<Item, u32>;
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

/// Everything the bus is configured with, see `I2c::i2c_configure`: what is asked for going in, what was actually
/// set coming back.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Copy, Clone)
)]
pub struct I2cConfig {
    pub mode: I2cMode,
    /// Bus voltage on adapters with level translators ([I2cCapabilities::io_levels]); `None` leaves it as it is going
    /// in (the only choice on a device without them), and is the answer of a device without them.
    pub io_level: Option<Volt>,
}

/// Bus mode, part of [I2cConfig]. Slave mode is `I2c::slave_events`, not a mode here.
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
    Master {
        speed: Hertz,
        monitor: bool,
        /// The longest a slave may hold SCL low at once (clock stretching) before the device aborts the transfer
        /// with [I2cError::ClockStretch], within [I2cCapabilities::stretch_timeout]. It bounds every wait on the
        /// bus, so a stuck SCL never hangs the device: a device always enforces a limit of its own. `None` leaves it
        /// as it is going in; the answer is always the limit in effect.
        stretch_timeout: Option<Second>,
    },
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
    /// Write these bytes; none is an address-only write where [I2cCapabilities::zero_length_write] says so.
    Write(MaxLen<RefVec<'i, u8>, MAX_WRITE_LEN>),
    /// Read this many bytes.
    Read(Bytes<u16>),
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

/// What the adapter supports, see `I2c::i2c_capabilities`.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct I2cCapabilities<'i> {
    /// Driver or controller, for the user: "STM32 I2C1", "RP2350 I2C0 + PIO monitor".
    pub driver_name: MaxLen<&'i str, MAX_NAME_LEN>,
    /// [I2cMode::Master] works.
    pub master_supported: bool,
    /// [I2cMode::MonitorOnly] and the `monitor` flag of [I2cMode::Master] work.
    pub monitor_supported: bool,

    /// Discrete IO levels the adapter can set (e.g. 1.8 V, 3.3 V, 5 V); empty if it has no level control.
    pub io_levels: MaxLen<RefVec<'i, Volt>, MAX_IO_LEVELS>,
    /// Lowest IO level of an adjustable supply, if the adapter has one.
    pub io_level_min: Option<Volt>,
    /// Highest IO level of an adjustable supply, if the adapter has one.
    pub io_level_max: Option<Volt>,
    /// `I2c::i2c_set_pullups` works.
    pub pullups: bool,

    /// Lowest SCL frequency.
    pub min_freq: Hertz,
    /// Highest SCL frequency.
    pub max_freq: Hertz,
    /// The only SCL frequencies the controller can do (e.g. 20, 100, 400 and 750 kHz), ascending; empty when it
    /// does any between [Self::min_freq] and [Self::max_freq] (as fine as its clock divider allows).
    pub speeds: MaxLen<RefVec<'i, Hertz>, MAX_SPEEDS>,
    /// Range of the master's clock stretch timeout ([I2cMode::Master]'s `stretch_timeout`); `None` when it can't
    /// be changed (the device still enforces its own, and answers it in `I2c::i2c_configure`).
    pub stretch_timeout: Option<I2cTimeRange>,

    /// Largest write this device takes, 0 to [MAX_WRITE_LEN] bytes.
    pub max_write_len: Bytes<u16, Range<0, { MAX_WRITE_LEN as i64 }>>,
    /// Largest read this device does, 0 to [MAX_READ_LEN] bytes.
    pub max_read_len: Bytes<u16, Range<0, { MAX_READ_LEN as i64 }>>,
    /// Longest write part of an [I2cReadKind::RepeatedStart] read this device takes, 0 to [MAX_WRITE_READ_LEN]
    /// bytes (0 without a master).
    pub max_write_read_len: Bytes<u8, Range<0, { MAX_WRITE_READ_LEN as i64 }>>,
    /// Most operations of one `I2c::i2c_transaction` this device takes, 0 to [MAX_OPS] (0 without a master).
    pub max_ops: Count<Op, u8, Range<0, { MAX_OPS as i64 }>>,

    /// 10-bit addresses work.
    pub ten_bit_addresses: bool,
    /// A write without data works (start, address, stop: probing an address, SMBus Quick Command), in
    /// `I2c::i2c_write` and as an empty [I2cOp::Write]. Without it, an empty write is `Unsupported`.
    pub zero_length_write: bool,
    /// `I2c::i2c_wiggle_clock` works.
    pub bus_recovery: bool,
}

/// A range of durations, both ends included.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Copy, Clone)
)]
pub struct I2cTimeRange {
    pub min: Second,
    pub max: Second,
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

    /// `transparent` (DER-18): a watch's slot costs exactly the plain `u32` it replaces, no extra length byte.
    #[test]
    fn watch_slot_is_exactly_a_plain_u32_on_the_wire() {
        let mut buf = [0u8; 8];
        let slot_bytes = WatchSlot(7).to_ww_bytes(&mut buf).unwrap();
        let mut buf2 = [0u8; 8];
        let plain_bytes = 7u32.to_ww_bytes(&mut buf2).unwrap();
        assert_eq!(slot_bytes, plain_bytes);
    }

    /// `i2c_scan` returns `RefVec<I2cAddress>` instead of a bare `RefVec<u8>`: since every element it finds is the
    /// one-byte `Seven` variant, the bytes are exactly the same (AGENTS.md: checked, not hand-derived).
    #[test]
    fn scan_addresses_cost_the_same_bytes_as_plain_u8() {
        let addrs = [
            I2cAddress::seven(0x50).unwrap(),
            I2cAddress::seven(0x60).unwrap(),
        ];
        let mut buf = [0u8; 8];
        let as_addresses = RefVec::Slice { slice: &addrs }
            .to_ww_bytes(&mut buf)
            .unwrap();

        let as_u8: [u8; 2] = [0x50, 0x60];
        let mut buf2 = [0u8; 8];
        let as_plain = RefVec::Slice { slice: &as_u8 }
            .to_ww_bytes(&mut buf2)
            .unwrap();

        assert_eq!(as_addresses, as_plain);
    }

    /// A match rule costs two bytes per byte read, no size marker per element.
    #[test]
    fn match_byte_is_two_bytes_on_the_wire() {
        assert_eq!(
            <I2cMatchByte as SerializeShrinkWrap>::ELEMENT_SIZE,
            ElementSize::Sized { size_bits: 16 }
        );
        let b = I2cMatchByte {
            mask: 0x81,
            value: 0x01,
        };
        let mut buf = [0u8; 4];
        let bytes = b.to_ww_bytes(&mut buf).unwrap();
        assert_eq!(bytes, &[0x81, 0x01]);
    }

    #[test]
    fn address_constructors_reject_out_of_range_values() {
        assert!(I2cAddress::seven(0x80).is_none());
        assert!(I2cAddress::ten(0x400).is_none());
    }
    #[derive_shrink_wrap(derive(Debug, PartialEq, Clone))]
    struct Reply<'i> {
        result: Result<u8, I2cError<'i>>,
        after: u8,
    }

    fn bytes_of<T: SerializeShrinkWrap>(value: &T) -> Vec<u8> {
        let mut buf = [0u8; 256];
        value.to_ww_bytes(&mut buf).unwrap().to_vec()
    }

    #[test]
    fn new_errors_round_trip_and_display() {
        for e in [
            I2cError::Nack {
                op: Index::new(2),
                at: Some(Index::new(5)),
            },
            I2cError::Nack {
                op: Index::new(0),
                at: None,
            },
            I2cError::ClockStretch,
            I2cError::BusStuck {
                scl_low: false,
                sda_low: true,
            },
        ] {
            let b = bytes_of(&e);
            assert_eq!(I2cError::from_ww_bytes(&b).unwrap(), e);
        }
        assert_eq!(
            I2cError::Nack {
                op: Index::new(1),
                at: Some(Index::new(0)),
            }
            .to_string(),
            "not acknowledged: operation 1, byte Some(Index<Byte>(0 u16)) (0: the address)"
        );
        assert_eq!(
            I2cError::BusStuck {
                scl_low: true,
                sda_low: false
            }
            .to_string(),
            "bus stuck: SCL low true, SDA low false"
        );
    }

    #[test]
    fn master_mode_with_stretch_timeout_round_trips() {
        let config = I2cConfig {
            mode: I2cMode::Master {
                speed: Hertz::kilo(400u16),
                monitor: false,
                stretch_timeout: Some(Second::milli(25u8)),
            },
            io_level: Some(Volt::milli(3300u16)),
        };
        let b = bytes_of(&config);
        assert_eq!(I2cConfig::from_ww_bytes(&b).unwrap(), config);
    }

    #[test]
    fn capabilities_round_trip() {
        let speeds = [Hertz::kilo(100u8), Hertz::kilo(400u16)];
        let caps = I2cCapabilities {
            driver_name: MaxLen::new("DW_apb_i2c").unwrap(),
            master_supported: true,
            monitor_supported: false,
            io_levels: MaxLen::new(RefVec::Slice { slice: &[] }).unwrap(),
            io_level_min: None,
            io_level_max: None,
            pullups: false,
            min_freq: Hertz::kilo(10u8),
            max_freq: Hertz::mega(1u8),
            speeds: MaxLen::new(RefVec::Slice { slice: &speeds }).unwrap(),
            stretch_timeout: Some(I2cTimeRange {
                min: Second::milli(1u8),
                max: Second::base(1u8),
            }),
            max_write_len: Bytes::<u16, _>::bounded(256),
            max_read_len: Bytes::<u16, _>::bounded(256),
            max_write_read_len: Bytes::<u8, _>::bounded(4),
            max_ops: Count::<_, u8, _>::bounded(8),
            ten_bit_addresses: true,
            zero_length_write: false,
            bus_recovery: true,
        };
        let b = bytes_of(&caps);
        let back = I2cCapabilities::from_ww_bytes(&b).unwrap();
        assert_eq!(*back.driver_name, "DW_apb_i2c");
        assert_eq!(
            back.speeds.iter().map(|s| s.unwrap()).collect::<Vec<_>>(),
            speeds
        );
        assert_eq!(back.stretch_timeout, caps.stretch_timeout);
        assert_eq!(
            (
                back.max_write_len.get(),
                back.max_read_len.get(),
                back.max_write_read_len.get(),
                back.max_ops.get()
            ),
            (256, 256, 4, 8)
        );
        assert!(back.ten_bit_addresses && !back.zero_length_write && back.bus_recovery);

        // a limit past its const is refused on decode: the byte of `max_ops` is the one that differs between 8 and
        // 9, and 17 there is one past MAX_OPS
        let nine = bytes_of(&I2cCapabilities {
            max_ops: Count::<_, u8, _>::bounded(9),
            ..caps.clone()
        });
        let at = b.iter().zip(&nine).position(|(a, b)| a != b).unwrap();
        let mut over = b.clone();
        over[at] = MAX_OPS as u8 + 1;
        assert!(I2cCapabilities::from_ww_bytes(&over).is_err());
        over[at] = MAX_OPS as u8;
        assert_eq!(
            I2cCapabilities::from_ww_bytes(&over).unwrap().max_ops.get(),
            16
        );
    }

    #[test]
    fn watch_limits_are_bounded_by_their_consts() {
        let caps = I2cWatchCapabilities {
            max_watches: Count::<_, u16, _>::bounded(MAX_WATCHES as u16),
            max_len: Bytes::<u8, _>::bounded(MAX_WATCH_LEN as u8),
            min_period: Second::milli(1u8),
        };
        let back = I2cWatchCapabilities::from_ww_bytes(&bytes_of(&caps)).unwrap();
        assert_eq!((back.max_watches.get(), back.max_len.get()), (512, 32));
        assert!(Count::<Watch, u16, Range<1, { MAX_WATCHES as i64 }>>::try_new(0).is_err());
        assert!(Count::<Watch, u16, Range<1, { MAX_WATCHES as i64 }>>::try_new(513).is_err());
        assert!(Bytes::<u8, Range<1, { MAX_WATCH_LEN as i64 }>>::try_new(33).is_err());
    }
}

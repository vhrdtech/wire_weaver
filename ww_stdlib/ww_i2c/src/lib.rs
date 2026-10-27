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

/// Most discrete SCL frequencies a device lists in [I2cCapabilities::speeds].
pub const MAX_SPEEDS: usize = 8;

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
        len: u16,
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
    fn i2c_scan() -> Result<MaxLen<RefVec<'i, u8>, MAX_SCAN>, I2cError<'i>>;

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
    fn i2c_wiggle_clock(count: u8) -> Result<I2cLinesState, I2cError<'i>>;

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
        offset: u32,
        data: MaxLen<RefVec<'i, u8>, MAX_WRITE_LEN>,
    ) -> Result<(), I2cError<'i>>;

    /// Reads `len` bytes of the register map from `offset` as the master sees it: what the master wrote, or what
    /// the host put there. A `I2c::slave_map_write` still waiting for the master's transaction to end shows once it
    /// is applied.
    /// `BufferTooBig` past [I2cSlaveCapabilities::map_len] or for `len` above [MAX_READ_LEN].
    #[optional(slave)]
    #[errors(BufferTooBig)]
    fn slave_map_read(
        offset: u32,
        len: u16,
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
    fn slave_clear_answers() -> u32;
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
    /// `InvalidArgument` for a period below the device's minimum or a mask or match rule not as long as the read,
    /// `BufferTooBig` for a read longer than this device watches, `WrongState` while not a master.
    #[errors(Common(InvalidArgument, WrongState), OutOfWatchSlots, BufferTooBig)]
    fn watch_add(watch: I2cWatchConfig<'i>) -> Result<u32, I2cError<'i>>;

    /// Replaces a watch's configuration; its previous result is forgotten, so the next read is reported. Errors as
    /// `I2cWatch::watch_add`.
    #[errors(Common(InvalidArgument, WrongState), InvalidWatchSlot, BufferTooBig)]
    fn watch_modify(slot: u32, watch: I2cWatchConfig<'i>) -> Result<(), I2cError<'i>>;

    /// Removes a watch.
    #[errors(InvalidWatchSlot)]
    fn watch_remove(slot: u32) -> Result<(), I2cError<'i>>;

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
    /// When `data[i] & bytes[i].mask == bytes[i].value` for every byte starts or stops being true (an alert bit set,
    /// a status reaching a state); one [I2cMatchByte] per byte read.
    Matches {
        bytes: MaxLen<RefVec<'i, I2cMatchByte>, MAX_WATCH_LEN>,
    },
}

/// One byte of an [I2cWatchReport::Matches] rule: the read byte matches when `byte & mask == value`. A `mask` of
/// `0x00` ignores the byte. Two bytes on the wire.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Eq, Copy, Clone),
    sized
)]
pub struct I2cMatchByte {
    pub mask: u8,
    pub value: u8,
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
    pub result: Result<MaxLen<RefVec<'i, u8>, MAX_WATCH_LEN>, I2cError<'i>>,
}

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
    /// Master: the longest a slave may hold SCL low at once (clock stretching) before the device aborts the transfer
    /// with [I2cError::ClockStretch], within [I2cCapabilities::stretch_timeout_min] ..
    /// [I2cCapabilities::stretch_timeout_max]. It bounds every wait on the bus, so a stuck SCL never hangs the
    /// device: a device always enforces a limit of its own. `None` leaves it as it is going in; the answer is the
    /// limit in effect, `None` only from a device older than this field.
    #[default = None]
    pub stretch_timeout: Option<Second>,
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
    Master { speed: Hertz, monitor: bool },
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
    /// The first byte on the bus that was not acknowledged: `Some(0)` the address, `Some(n)` the n-th data byte
    /// (`bytes[n - 1]`); the same count as [I2cError::Nack]'s `at`.
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
    pub monitor_supported: bool,
    /// 10-bit addresses work.
    pub ten_bit_addresses: bool,
    /// `I2c::i2c_set_pullups` works.
    pub pullups: bool,
    /// `I2c::i2c_wiggle_clock` works.
    pub bus_recovery: bool,
    /// The only SCL frequencies the controller can do (e.g. 20, 100, 400 and 750 kHz), ascending; empty when it
    /// does any between [Self::min_freq] and [Self::max_freq] (as fine as its clock divider allows).
    #[default = Default::default()]
    pub speeds: MaxLen<RefVec<'i, Hertz>, MAX_SPEEDS>,
    /// Most operations of one `I2c::i2c_transaction` this device takes, at most [MAX_OPS].
    #[default = MAX_OPS as u16]
    pub max_ops: u16,
    /// Longest write part of an [I2cReadKind::RepeatedStart] read this device takes, at most [MAX_WRITE_READ_LEN].
    #[default = MAX_WRITE_READ_LEN as u16]
    pub max_write_read_len: u16,
    /// A write without data works (start, address, stop: probing an address, SMBus Quick Command), in
    /// `I2c::i2c_write` and as an empty [I2cOp::Write]. Without it, an empty write is `Unsupported`.
    #[default = false]
    pub zero_length_write: bool,
    /// Shortest [I2cConfig::stretch_timeout] this device sets; `None` when it can't be changed.
    #[default = None]
    pub stretch_timeout_min: Option<Second>,
    /// Longest [I2cConfig::stretch_timeout] this device sets; `None` when it can't be changed.
    #[default = None]
    pub stretch_timeout_max: Option<Second>,
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
        pointer_bytes: u8,
        len: u32,
        page: Option<u16>,
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
    pub pointer: Option<u32>,
    /// The data bytes: what the master wrote (pointer bytes included), or what this device sent.
    pub bytes: MaxLen<RefVec<'i, u8>, MAX_RECORD_LEN>,
    /// Bytes of a read sent as the fill byte because no answer was there (or came too late); 0 when all came from
    /// an answer or the map.
    pub filled: u16,
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
    pub map_len: u32,
    /// Widest register pointer, in bytes.
    pub max_pointer_bytes: u8,
    /// Answers the queue holds, 0 if there is none ([I2cSlaveAnswers::Queue] unsupported).
    pub queue_len: u32,
    /// Longest [I2cSlaveUnderrun::Stretch], `None` if the device can't stretch while it waits for the host.
    pub max_stretch: Option<Second>,
    /// [I2cSlaveUnderrun::Nack] works.
    pub nack_when_empty: bool,
    /// A 10-bit slave address works.
    pub ten_bit_address: bool,
    /// Fastest SCL the slave keeps up with.
    pub max_freq: Hertz,
}

/// Errors of every trait in this crate: the common ones first, then what is specific to I2C. Each method lists the
/// variants it returns.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Eq, Clone),
    ww_repr = unib32,
    display
)]
pub enum I2cError<'i> {
    /// {0}
    #[common]
    Common(CommonError<'i>),
    /// Not acknowledged: the address, or a data byte of a write (`at`), in operation `op`.
    ///
    /// `op` is 0 for `I2c::i2c_write` and a plain `I2c::i2c_read`; for an [I2cReadKind::RepeatedStart] read 0 is
    /// the write part and 1 the read part; for `I2c::i2c_transaction` the index of the [I2cOp]. `at` counts bytes on
    /// the bus within that operation as [I2cRecord::nack_at] does: `Some(0)` the address, `Some(n)` the n-th data
    /// byte (`data[n - 1]`, so `n - 1` bytes went through), `None` when the controller can't tell (and from a device
    /// older than these fields). A slave only refuses data on writes: in a read it is the master that doesn't
    /// acknowledge the last byte, which is no error.
    #[display("not acknowledged: operation {op}, byte {at:?} (0: the address)")]
    Nack {
        #[default = 0]
        op: u8,
        #[default = None]
        at: Option<u16>,
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
    /// A slave held SCL low longer than [I2cConfig::stretch_timeout] at once; the transfer was aborted with a stop.
    #[display("a slave held SCL low longer than the stretch timeout")]
    ClockStretch,
    /// A line is held low when the master wants to start (`scl_low`, `sda_low`: which one), so it can't: a slave
    /// stuck mid-byte, missing pull-ups or a short. The device doesn't recover on its own; `I2c::i2c_wiggle_clock`
    /// frees a stuck slave.
    #[display("bus stuck: SCL low {scl_low}, SDA low {sda_low}")]
    BusStuck { scl_low: bool, sda_low: bool },
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
    /// The shapes of 0.1.0 before STD-52 (a unit `Nack`, no clock stretch timeout, fewer capabilities), to check
    /// that old hosts and old firmware read what the new version writes and the other way round.
    mod v0_1 {
        use wire_weaver::prelude::*;
        use ww_si::{Hertz, Volt};

        #[derive_shrink_wrap(derive(Debug, PartialEq, Eq, Clone), ww_repr = unib32)]
        pub enum I2cError<'i> {
            Common(CommonError<'i>),
            Nack,
            Arbitration,
            Bus,
            Crc,
            Overrun,
            BufferTooBig,
            OutOfWatchSlots,
            InvalidWatchSlot,
        }

        /// A reply as a method sends it, with a field after the error, so a skipped variant must not lose it.
        #[derive_shrink_wrap(derive(Debug, PartialEq, Clone))]
        pub struct Reply<'i> {
            pub result: Result<u8, I2cError<'i>>,
            pub after: u8,
        }

        #[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Copy, Clone))]
        pub struct I2cConfig {
            pub mode: super::I2cMode,
            pub io_level: Option<Volt>,
        }

        #[derive_shrink_wrap(derive(Debug, Clone))]
        pub struct I2cCapabilities<'i> {
            pub io_levels: MaxLen<RefVec<'i, Volt>, 8>,
            pub io_level_min: Option<Volt>,
            pub io_level_max: Option<Volt>,
            pub max_write_len: u32,
            pub max_read_len: u32,
            pub min_freq: Hertz,
            pub max_freq: Hertz,
            pub driver_name: MaxLen<&'i str, 32>,
            pub master_supported: bool,
            pub monitor_supported: bool,
            pub ten_bit_addresses: bool,
            pub pullups: bool,
            pub bus_recovery: bool,
        }
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
            I2cError::Nack { op: 2, at: Some(5) },
            I2cError::Nack { op: 0, at: None },
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
            I2cError::Nack { op: 1, at: Some(0) }.to_string(),
            "not acknowledged: operation 1, byte Some(0) (0: the address)"
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

    /// An old host reads a new device's `Nack` as its plain `Nack` and the new errors as `Unknown(n)`, and the field
    /// after the error is still read.
    #[test]
    fn old_host_reads_new_errors() {
        let check = |e: I2cError<'static>, old: v0_1::I2cError<'static>| {
            let b = bytes_of(&Reply {
                result: Err(e),
                after: 0xAB,
            });
            assert_eq!(
                v0_1::Reply::from_ww_bytes(&b).unwrap(),
                v0_1::Reply {
                    result: Err(old),
                    after: 0xAB,
                }
            );
        };
        check(
            I2cError::Nack {
                op: 3,
                at: Some(17),
            },
            v0_1::I2cError::Nack,
        );
        check(I2cError::ClockStretch, v0_1::I2cError::Unknown(9));
        check(
            I2cError::BusStuck {
                scl_low: true,
                sda_low: true,
            },
            v0_1::I2cError::Unknown(10),
        );
    }

    /// A new host reads an old device's `Nack` with nothing known about where.
    #[test]
    fn new_host_reads_old_nack() {
        let b = bytes_of(&v0_1::Reply {
            result: Err(v0_1::I2cError::Nack),
            after: 7,
        });
        assert_eq!(
            Reply::from_ww_bytes(&b).unwrap(),
            Reply {
                result: Err(I2cError::Nack { op: 0, at: None }),
                after: 7
            }
        );
    }

    fn master_400k() -> I2cMode {
        I2cMode::Master {
            speed: Hertz::kilo(400u16),
            monitor: false,
        }
    }

    #[test]
    fn config_with_stretch_timeout_round_trips_and_old_readers_skip_it() {
        let config = I2cConfig {
            mode: master_400k(),
            io_level: Some(Volt::milli(3300u16)),
            stretch_timeout: Some(Second::milli(25u8)),
        };
        let b = bytes_of(&config);
        assert_eq!(I2cConfig::from_ww_bytes(&b).unwrap(), config);
        // an old device reads the mode and the level, and leaves its stretch timeout as it is
        assert_eq!(
            v0_1::I2cConfig::from_ww_bytes(&b).unwrap(),
            v0_1::I2cConfig {
                mode: master_400k(),
                io_level: Some(Volt::milli(3300u16)),
            }
        );
        // an old device's answer has no stretch timeout
        let old = bytes_of(&v0_1::I2cConfig {
            mode: master_400k(),
            io_level: None,
        });
        assert_eq!(
            I2cConfig::from_ww_bytes(&old).unwrap(),
            I2cConfig {
                mode: master_400k(),
                io_level: None,
                stretch_timeout: None,
            }
        );
    }

    #[test]
    fn capabilities_new_fields_round_trip_and_default_from_old_devices() {
        let speeds = [Hertz::kilo(100u8), Hertz::kilo(400u16)];
        let caps = I2cCapabilities {
            io_levels: MaxLen::new(RefVec::Slice { slice: &[] }).unwrap(),
            io_level_min: None,
            io_level_max: None,
            max_write_len: 256,
            max_read_len: 256,
            min_freq: Hertz::kilo(10u8),
            max_freq: Hertz::mega(1u8),
            driver_name: MaxLen::new("DW_apb_i2c").unwrap(),
            master_supported: true,
            monitor_supported: false,
            ten_bit_addresses: true,
            pullups: false,
            bus_recovery: true,
            speeds: MaxLen::new(RefVec::Slice { slice: &speeds }).unwrap(),
            max_ops: 8,
            max_write_read_len: 4,
            zero_length_write: true,
            stretch_timeout_min: Some(Second::milli(1u8)),
            stretch_timeout_max: Some(Second::base(1u8)),
        };
        let b = bytes_of(&caps);
        let back = I2cCapabilities::from_ww_bytes(&b).unwrap();
        assert_eq!(
            back.speeds.iter().map(|s| s.unwrap()).collect::<Vec<_>>(),
            speeds
        );
        assert_eq!(back.max_ops, 8);
        assert_eq!(back.max_write_read_len, 4);
        assert!(back.zero_length_write);
        assert_eq!(back.stretch_timeout_min, caps.stretch_timeout_min);
        assert_eq!(back.stretch_timeout_max, caps.stretch_timeout_max);
        // an old host reads the fields it knows
        let old = v0_1::I2cCapabilities::from_ww_bytes(&b).unwrap();
        assert_eq!(old.max_freq, caps.max_freq);
        assert!(old.bus_recovery);

        // a new host reads an old device's capabilities with the crate's bounds and nothing else promised
        let old_bytes = bytes_of(&v0_1::I2cCapabilities {
            io_levels: MaxLen::new(RefVec::Slice { slice: &[] }).unwrap(),
            io_level_min: None,
            io_level_max: None,
            max_write_len: 64,
            max_read_len: 64,
            min_freq: Hertz::kilo(10u8),
            max_freq: Hertz::kilo(400u16),
            driver_name: MaxLen::new("old").unwrap(),
            master_supported: true,
            monitor_supported: false,
            ten_bit_addresses: false,
            pullups: false,
            bus_recovery: false,
        });
        let new = I2cCapabilities::from_ww_bytes(&old_bytes).unwrap();
        assert_eq!(new.max_write_len, 64);
        assert!(new.speeds.is_empty());
        assert_eq!(new.max_ops, MAX_OPS as u16);
        assert_eq!(new.max_write_read_len, MAX_WRITE_READ_LEN as u16);
        assert!(!new.zero_length_write);
        assert_eq!(new.stretch_timeout_min, None);
        assert_eq!(new.stretch_timeout_max, None);
    }
}

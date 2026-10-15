# ww_i2c

`ww_i2c` is a global WireWeaver trait family for an I2C bus reachable through a device: a USB adapter, a dev board,
or an instrument. A host can configure the bus, run write/read/transaction calls against a slave address, scan the
bus, see and recover stuck lines, let the device poll a few registers on its own ([`I2cWatch`](#i2cwatch)), or watch
every transaction on the bus without taking part in it ([`I2cMonitor`](#i2cmonitor)). It is `no_std` by default and
depends only on `wire_weaver` and `ww_si` (for the `Second` and `Volt` reference types).

Add it to a device API crate with:

```toml
[dependencies]
ww_i2c = "0.1"
```

## Traits

| Trait        | What it is for                                                                          | Who implements it            |
|--------------|------------------------------------------------------------------------------------------|-------------------------------|
| `I2c`        | Configure the bus, write, read, write-read, transactions, scan, bus lines and recovery.  | Every I2C master             |
| `I2cWatch`   | Autonomous reads: the device repeats configured reads and reports only changes.          | Adapters that offload polling |
| `I2cMonitor` | Every transaction on the bus as a stream, without taking part in it.                     | Sniffers, debug adapters      |

`I2cMonitor` is its own trait because it is its own function: a sniffer may have no master at all, and most
masters can't sniff. A host finds out what is there through introspection, and what `I2c` supports through
[`I2c::i2c_capabilities`](#i2c).

```rust
#[ww_api_root]
trait MyAdapter {
    ww_impl!(i2c: ww_i2c::I2c);
    ww_impl!(i2c_watch: ww_i2c::I2cWatch);
    ww_impl!(i2c_monitor: ww_i2c::I2cMonitor);
}
```

### `I2c`

The most common call of a register-based sensor driver is a write-read: `i2c_read` with
[`I2cReadKind::RepeatedStart`](#i2creadkind) (register address out, data in, one transaction). Several calls in one
round trip are a multi-call of the client, not a method here - use `i2c_transaction` when the device itself must do
several operations without releasing the bus in between (repeated starts).

```rust
#[ww_trait]
pub trait I2c {
    fn i2c_capabilities() -> I2cCapabilities<'i>;
    fn i2c_configure(mode: I2cMode, io_level: Option<Volt>) -> Result<Hertz, I2cError>;
    fn i2c_write(addr: I2cAddress, data: MaxLen<RefVec<'i, u8>, MAX_WRITE_LEN>, delay_after: Option<Second>)
        -> Result<I2cTransactionTimeRange, I2cError>;
    fn i2c_read(addr: I2cAddress, kind: I2cReadKind<'i>, len: u16, delay_after: Option<Second>)
        -> Result<I2cReadEnvelope<'i>, I2cError>;
    fn i2c_scan() -> Result<MaxLen<RefVec<'i, u8>, MAX_SCAN>, I2cError>;
    fn i2c_transaction(addr: I2cAddress, ops: MaxLen<RefVec<'i, I2cOp<'i>>, MAX_OPS>)
        -> Result<I2cTransactionResult<'i>, I2cError>;
    fn i2c_lines_state() -> I2cLinesState;
    fn i2c_wiggle_clock(count: u8) -> Result<I2cLinesState, I2cError>;
    fn i2c_set_pullups(enabled: bool) -> Result<(), I2cError>;
    fn i2c_pullups() -> bool;
}
```

* `i2c_capabilities` - static description of what the adapter supports: modes, frequency range, IO levels, buffer
  sizes, 10-bit addresses, pull-ups, bus recovery; see `I2cCapabilities` under [Types](#types).
* `i2c_configure` - sets the bus mode and, on adapters with level translators, the bus voltage (`None` leaves it as
  it is); returns the SCL frequency actually set (never above the one asked for).
* `i2c_write` - start, address, data, stop; `delay_after` makes the next transfer to the *same* address wait (an
  EEPROM page write needs ~5 ms) while transfers to other addresses go ahead meanwhile.
* `i2c_read` - plain read, or a register read (write a few bytes, repeated start, read) through
  [`I2cReadKind`](#i2creadkind).
* `i2c_scan` - 7-bit addresses that acknowledged a one-byte read, ascending, skipping the reserved ranges
  (`0x00..=0x07`, `0x78..=0x7F`).
* `i2c_transaction` - several [`I2cOp`](#i2cop) writes/reads to one address with repeated starts in between and one
  stop at the end, like `embedded_hal::i2c::I2c::transaction`; the bytes of all reads come back concatenated, in
  order.
* `i2c_lines_state` - levels of SCL and SDA now; both high on an idle bus, a line held low means a slave stuck
  mid-byte, missing/unpowered pull-ups, or a short.
* `i2c_wiggle_clock` - clocks SCL up to `count` times (9 is enough for any stuck byte) until SDA is released, then
  sends a stop; returns the line levels afterwards (bus recovery).
* `i2c_set_pullups` / `i2c_pullups` - the adapter's own pull-up resistors on SCL and SDA, on adapters that have
  switchable ones (see `I2cCapabilities::pullups`).

### `I2cWatch`

Autonomous reads: the device repeats configured reads on its own and reports only what the host asked for (every
result, a change under a mask, or a mask/value match starting or stopping), so the host doesn't have to poll over
the link. A device implementing it also implements `I2c` (it needs a master). Host transactions have priority: the
device runs a watch read only when no host transaction is waiting, so host calls are spliced in between autonomous
ones and see no more delay than one watch read.

```rust
#[ww_trait]
pub trait I2cWatch {
    #[counted]
    stream!(changes: I2cWatchEvent<'i>);
    fn watch_capabilities() -> I2cWatchCapabilities;
    fn watch_add(watch: I2cWatchConfig<'i>) -> Result<u32, I2cError>;
    fn watch_modify(slot: u32, watch: I2cWatchConfig<'i>) -> Result<(), I2cError>;
    fn watch_remove(slot: u32) -> Result<(), I2cError>;
    fn watch_list() -> MaxLen<RefVec<'i, u32>, MAX_WATCHES>;
}
```

* `watch_add` / `watch_modify` / `watch_remove` / `watch_list` - manage watch slots (at most
  `I2cWatchCapabilities::max_watches`, see [Types](#types)); modifying a watch forgets its previous result, so the
  next read is always reported.
* `changes` - results, in the order the watches were read, as [`I2cWatchEvent`](#types). A counted stream (unit:
  result): the host opens it by calling it, the event head carries when each read started and how many results the
  device dropped ([stream delivery](../api/stream_reliability.md)).

`I2cWatchReport` (part of `I2cWatchConfig`) decides when a result is worth a report:

* `Always` - every read.
* `Changed { mask }` - when `data & mask` differs from the *previous reported* read's (`mask` as long as the read;
  all-ones to see any change).
* `Matches { bytes }` - when `data[i] & bytes[i].mask == bytes[i].value` for every byte starts or stops being true
  (an alert bit set, a status reaching a state) - a transition, not every read while it holds. One `I2cMatchByte
  { mask, value }` per byte read, two bytes each on the wire; a `mask` of `0x00` ignores that byte.

An error is reported once, when the watch starts to fail, and the next successful read is reported unconditionally
whatever the rule - and that recovery read becomes the new baseline, so a later unchanged read goes quiet again
instead of being reported a second time. `tests/io_i2c` exercises all three rules plus this error/recovery sequence
end to end against a mock server.

### `I2cMonitor`

Watches the bus without taking part: every transaction becomes one `I2cRecord` on the `records` stream. The same
function as `I2cMode::MonitorOnly` (or the `monitor` flag of `I2cMode::Master`, to watch the adapter's own transfers
too), for devices that only sniff and implement no `I2c`.

```rust
#[ww_trait]
pub trait I2cMonitor {
    #[stream(I2cRecord<'i>, counted)]
    fn records(io_level: Option<Volt>) -> Result<(), I2cError>;
}
```

`records` is a counted [stream function](../api/streams.md#opening-a-stream): opening it starts monitoring (at
`io_level`, as in `i2c_configure`; the adapter's pull-ups stay as they are, the watched bus has its own), closing it
stops. Its unit is a transaction: the event head carries the time of each start condition and exactly how many
records the device dropped and where ([stream delivery](../api/stream_reliability.md)).

## Addresses

`I2cAddress` is 7-bit (`0x50` for a 24C02 EEPROM, not `0xA0`) or 10-bit, as in the I2C specification and
embedded-hal (`SevenBitAddress` / `TenBitAddress`): `I2cAddress::seven(0x50)`, `I2cAddress::ten(0x350)`, both
returning `None` above the respective range. It is bit-packed (SW-48, sub-byte fields): a 7-bit address takes
**one byte** on the wire, exactly as a plain `u8` would, and a 10-bit one 11 bits (2 bytes) - `ww_i2c`'s own test
suite checks both sizes against a real `to_ww_bytes()` call. An adapter without 10-bit support answers
`I2cError::Unsupported` (see `I2cCapabilities::ten_bit_addresses`). `i2c_scan` still only returns 7-bit addresses (a
10-bit scan is rarely useful and would need 1024 single-byte reads).

## Sizes

Every byte buffer is bounded (`MAX_WRITE_LEN`, `MAX_READ_LEN`, `MAX_WRITE_READ_LEN`, `MAX_OPS`, `MAX_SCAN`,
`MAX_RECORD_LEN`, `MAX_WATCHES`, `MAX_WATCH_LEN`, `MAX_NAME_LEN`, `MAX_IO_LEVELS`), so a device knows the largest
request it has to take and the largest reply it has to build at compile time. A device may support less: its limits
are in `I2cCapabilities` / `I2cWatchCapabilities`. Raising a bound later is a compatible change, lowering it is not
(see [evolution rules](../evolution/rules.md)).

## Time

Every transfer reports when it started and ended (`I2cTransactionTimeRange`), and every monitor record and watch
result when it happened (the time in the event head of its counted stream), as `ww_si::Second`s of device time: time since the device's clock started, in the prefix
and number type the device picks (usually a `u32` of microseconds). A fixed-width value wraps (a `u32` of µs every
71.6 minutes) and the host unwraps it. All timestamps of one device come from the same clock, so I2C transfers line
up with the device's other data.

## Types

* `I2cMode` - `Disabled` (pins released), `Master { speed: Hertz, monitor }`, `Slave { addr, monitor }` (data paths
  for slave mode are not defined yet), `MonitorOnly`.
* `I2cReadKind<'i>` - `Plain` (start, address+read, `len` bytes, stop) or `RepeatedStart { write }` (start,
  address+write, `write`, repeated start, address+read, `len` bytes, stop), bounded by `MAX_WRITE_READ_LEN`.
* `I2cOp<'i>` - `Write(MaxLen<RefVec<'i, u8>, MAX_WRITE_LEN>)` or `Read(u16)`, one operation of `i2c_transaction`.
* `I2cTransactionTimeRange` - `start` / `end: Second` (start and stop condition).
* `I2cReadEnvelope<'i>` - `duration: I2cTransactionTimeRange`, `data: MaxLen<RefVec<'i, u8>, MAX_READ_LEN>`.
* `I2cTransactionResult<'i>` - `duration`, `read` (every `I2cOp::Read`'s bytes, concatenated in order).
* `I2cLinesState` - `is_scl_high` / `is_sda_high: bool`, `scl` / `sda: Option<Volt>` (measured level, where the
  adapter can).
* `I2cCapabilities<'i>` - `io_levels`, `io_level_min` / `io_level_max`, `max_write_len`, `max_read_len`,
  `min_freq` / `max_freq: Hertz`, `driver_name` (bounded by `MAX_NAME_LEN`), `master_supported` /
  `slave_supported` / `monitor_supported`, `ten_bit_addresses`, `pullups`, `bus_recovery`.
* `I2cWatchCapabilities` - `max_watches: u32` (at most `MAX_WATCHES`, 512), `max_len` (at most `MAX_WATCH_LEN`), `min_period`.
* `I2cWatchConfig<'i>` - `addr`, `kind: I2cReadKind<'i>`, `len`, `period: Second`, `report: I2cWatchReport<'i>`.
* `I2cWatchReport<'i>` - `Always`, `Changed { mask }`, `Matches { bytes }`, see [`I2cWatch`](#i2cwatch) above.
* `I2cMatchByte` - `mask`, `value: u8`, one byte of a `Matches` rule; `sized`, two bytes.
* `I2cWatchEvent<'i>` - `slot`, `result: Result<MaxLen<RefVec<'i, u8>, MAX_WATCH_LEN>, I2cError>`.
* `I2cRecord<'i>` - `end: Second` (stop or repeated start; the start condition is the head's time), `addr`, `read: bool`, `kind: I2cRecordKind`, `nack_at: Option<u16>`
  (index of the first byte not acknowledged, `Some(0)` = the address), `bytes` (without the address).
* `I2cRecordKind` - `Complete`, `RepeatedStart` (the next record continues the transaction), `Truncated` (more than
  `MAX_RECORD_LEN` bytes), `BusError`.
* `I2cError` - `UnsupportedMode`, `WrongMode`, `UnsupportedSpeed`, `OutOfCycleSlots`, `InvalidCycleSlot`,
  `MalformedRequest`, `BufferTooBig`, `UnsupportedIoLevel`, `Bus`, `Arbitration`, `Nack`, `Timeout`, `Crc`,
  `Overrun`, `ZeroLengthTransfer`, `InUse`, `NoIoLevel`, `Refused`, `Unsupported`; unsized (`unib32` discriminant),
  plain for now (see [below](#errors)).

All enums and structs are behind `#[derive_shrink_wrap]` with `owned(feature = "std")` and
`cfg_attr_borrowed(feature = "defmt", derive(defmt::Format))`, so owned (`*Owned`) variants and `defmt::Format` are
available by enabling the crate's `std` / `defmt` features.

## Errors

Both `I2c` and `I2cWatch` return `I2cError`; every method's rustdoc lists the variants it actually returns. How
errors evolve (variants added in newer versions, per-method error lists, errors that carry a reason) is still open
for `ww_stdlib`; until then `I2cError` stays a plain enum.

## Composing into a device API, and testing (`tests/io_i2c`)

A device exposes the traits it implements on its API root, see `tests/io_i2c_api`:

```rust
#[ww_api_root]
pub trait IoI2cTest {
    ww_impl!(i2c: ww_i2c::I2c);
    ww_impl!(i2c_watch: ww_i2c::I2cWatch);
    ww_impl!(i2c_monitor: ww_i2c::I2cMonitor);
}
```

`tests/io_i2c` runs a mock server implementing all three traits on `ww_device::Server` (through
`tests_common::start_device`, the same in-process harness `tests/credit_sink` and `tests/streams` use), simulating
a tiny bus of addressable byte registers in a shared `Model`. The generated client drives it through the real host
event loop end to end:

* `i2c_write_read_roundtrip`, `i2c_nack_and_zero_length`, `i2c_scan_and_lines` - `I2c` write/read/scan/lines/pullups,
  and the `Nack` / `ZeroLengthTransfer` error paths.
* `i2c_monitor_records_writes_and_reads` - opening `I2cMonitor::records` turns transactions into `I2cRecord`s on it.
* `i2c_watch_always_reports_every_read`, `i2c_watch_changed_reports_only_on_change`,
  `i2c_watch_matches_reports_on_transition_only`, `i2c_watch_error_reported_once_then_recovery_always_reports`,
  `i2c_watch_modify_remove_and_list` - the three `I2cWatchReport` rules, the error-once-then-recovery sequence, and
  slot management.

Since nothing drives a real periodic timer in the mock, the test API adds one test-only method,
`tick_watches() -> u8`, that runs a single watch-read cycle on demand (applying `I2cWatchReport` exactly like a
real firmware's polling loop would) and returns how many watches reported - a real device would call the same
internal logic from its own timer or event loop instead.

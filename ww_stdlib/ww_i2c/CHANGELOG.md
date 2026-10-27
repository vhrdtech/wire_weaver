## Unreleased

Draft of the IO trait family (STD-13), under review; not published yet, so no version bump.

### ⚠️ Breaking

- Master errors and limits (STD-52), breaking on the wire and in Rust (nothing released or deployed yet):
    - `I2cError::Nack` is `Nack { op: u8, at: Option<u16> }`: which operation (`0` for `i2c_write` and a plain read,
      `0`/`1` for the write/read part of a `RepeatedStart` read, the index into `ops` for `i2c_transaction`), and
      what was not acknowledged, counted as `I2cRecord::nack_at`: `Some(0)` the address, `Some(n)` the n-th data
      byte, `None` unknown. Match `Nack { .. }`, build it with both fields.
    - `I2cMode::Master` gets `stretch_timeout: Option<Second>`: the longest a slave may hold SCL low at once before
      `I2cError::ClockStretch`. `None` leaves it as it is, the answer of `i2c_configure` always has the limit in
      effect (a device always enforces one).
    - `I2cCapabilities` is regrouped by topic (what it is, IO level, timing, sizes, features), its sizes
      `max_write_len` / `max_read_len` are `u16` (was `u32`, like every other length in the crate), and it gains
      `speeds`, `stretch_timeout: Option<I2cTimeRange>` (new type, `min` / `max`), `max_write_read_len`, `max_ops`
      and `zero_length_write` (below). Callers: update struct literals.
    - `I2cSlaveCapabilities::ten_bit_address` is `ten_bit_addresses`, as in `I2cCapabilities`.
    - Callers to adapt: usb_io_fw (`b200_board::slots::i2c`, the simulator in `b200_usb_io_client`, the firmware's
      `api/i2c.rs`) and typec_dongle (`donguru_api`, `fw/donguru/src/ww/i2c.rs`, `donguru-core` mock server).

- `I2cMode::Slave` and `I2cCapabilities::slave_supported` are removed (STD-35): slave mode is the new `slave` group of
  `I2c` (below), opened with `I2c::slave_events`; whether a device has it is in its introspection and
  `I2c::slave_capabilities`. `I2cMode::MonitorOnly` moves to discriminant 2.

- `I2cError<'i>`: the first variant is `#[common] Common(CommonError<'i>)` (STD-16): `Unsupported`, `InvalidArgument`,
  `InUse`, `Busy`, `WrongState`, `NoPower`, `Refused`, `Timeout` come from `ww_error::CommonError` instead of the
  crate's `UnsupportedMode`, `WrongMode`, `UnsupportedSpeed`, `MalformedRequest`, `UnsupportedIoLevel`, `Timeout`,
  `ZeroLengthTransfer`, `InUse`, `NoIoLevel`, `Refused`, `Unsupported`; `OutOfCycleSlots` / `InvalidCycleSlot` are
  `OutOfWatchSlots` / `InvalidWatchSlot`. It is no longer `self_describing` (newer variants read as `Unknown(n)`),
  carries a lifetime (`I2cErrorOwned` on hosts), has `Display` from its doc lines, and every method lists its variants
  with `#[errors(..)]`. Devices return `CommonError::Unsupported.into()` (or `?` a helper returning `CommonError`).
- `i2c_configure(config: I2cConfig) -> Result<I2cConfig, I2cError>` takes `I2cConfig { mode, io_level }` instead of
  two arguments and answers what was actually set (the SCL frequency inside the mode, the IO level in effect)
  instead of a bare frequency (STD-13).
- `I2cWatchReport::Matches { bytes }` holds one `I2cMatchByte { mask, value }` per byte read (a `sized` pair, two
  bytes) instead of two parallel `mask` and `value` vectors that had to be the same length.
- `I2cWatch` holds up to `MAX_WATCHES = 512` watches (was 16): `I2cWatchCapabilities::max_watches` is a `u32`, and
  slots are `u32` in `watch_add`, `watch_modify`, `watch_remove`, `watch_list` and `I2cWatchEvent::slot`.
- Frequencies are `ww_si::Hertz` (STD-13): `I2cMode::Master { speed, .. }` instead of `speed_hz: u32`,
  `i2c_configure` returns the actual `Hertz`, `I2cCapabilities::min_freq` / `max_freq` instead of
  `min_frequency_hz` / `max_frequency_hz`. `I2cMode` is no longer `self_describing` (it holds a `Hertz`) and gets
  `Unknown(u32)`.
- Addresses are `I2cAddress { Seven(u8), Ten(u16) }` instead of `u8` (10-bit addressing; `From<u8>` gives the
  7-bit one).
- `i2c_configure` takes `io_level: Option<Volt>` instead of `io_level_mv: u16` (`None` leaves the level as it is).
- `i2c_write` data and `i2c_scan` results are `MaxLen<RefVec<'i, u8>, N>` instead of `Vec<u8>` (`MAX_WRITE_LEN`,
  `MAX_SCAN`), so a `no_alloc` device can serve them; `I2cReadKind::RepeatedStart::write` is bounded too.
- `delay_after` is `Option<Second>` instead of a `u32` without a unit.
- `RawTimestamp` is gone: `I2cTransactionTimeRange` holds `ww_si::Second` device time.
- `I2cError` gains `InUse`, `NoIoLevel`, `Refused`, `Unsupported`.
- `I2cReadKind` (and the new `I2cOp`, `I2cWatchReport`, `I2cRecordKind`) get `Unknown(u32)` from
  `derive_shrink_wrap` (SW-47): a variant from a newer version of this crate is read into it instead of failing.
  Exhaustive matches need an `Unknown(_)` arm; a device answers it with `I2cError::Unsupported`. Same wire.
- `I2cCapabilities` lists IO levels as `Volt`s, bounds `driver_name`, adds `ten_bit_addresses`, `pullups`,
  `bus_recovery`.

### 🚀 Features

- Master errors and limits (STD-52), besides the breaking changes above:
    - `I2cError::ClockStretch`: a slave held SCL low longer than the master's `stretch_timeout`.
    - `I2cError::BusStuck { scl_low, sda_low }`: a line held low before the start. Devices don't recover on their
      own; `i2c_wiggle_clock` does, on request, clocking at the bus speed (at most 100 kHz) until SDA is released,
      or answers `Common(NoPower)` when the bus has no level to drive. `Common(Timeout)` is now only the call's own
      deadline (e.g. another master holds the bus).
    - `I2cCapabilities::speeds` (a controller's fixed SCL frequencies, `MAX_SPEEDS`), `stretch_timeout`,
      `max_write_read_len`, `max_ops`, `zero_length_write` (address-only writes; `Unsupported` without).
    - Stated semantics: `i2c_transaction` puts a repeated start and the address before every operation (Linux
      `I2C_RDWR`), it does not merge adjacent operations as embedded-hal does (docs say how an embedded-hal adapter
      merges); `i2c_configure` answers the highest SCL the hardware can do that is not above the request; a data
      NACK only happens on writes. `i2c_scan` lists `Arbitration`, `i2c_write` / `i2c_read` / `i2c_transaction` /
      `i2c_scan` list `ClockStretch` and `BusStuck` in `#[errors]`.
    - Tests: round trips of the new error, mode and capabilities shapes in `ww_i2c`, and `tests/io_i2c` end to end
      (data NACK in a transaction, stretch timeout, stuck bus and recovery).

- `I2c` is `#[claim(exclusive)]` (SRV-34): a client can claim a controller through the hub. Wire compatible, only the introspection data changes.
- Slave mode (STD-35): an `#[optional(slave)]` group in `I2c` (`omit = ["i2c.slave"]` without it).
  `slave_events(I2cSlaveConfig)` is a counted stream function: opening it makes the device a slave at
  `I2cSlaveConfig::addr`, and it reports every transaction as `I2cSlaveEvent::Transfer(I2cSlaveTransfer)` (bytes,
  register pointer, fill bytes, stretch time) and `I2cSlaveEvent::ReadWaiting` when the master waits for an answer.
  Answers come from a register map (`I2cSlaveAnswers::Registers`, filled with `slave_map_write`, read with
  `slave_map_read`) or from the `slave_answers` credit sink (`I2cSlaveAnswers::Queue`, cleared with
  `slave_clear_answers`), with `I2cSlaveUnderrun` (`Fill`, `Stretch { max, fill }`, `Nack`) for a host that is too
  slow; `slave_capabilities` answers `I2cSlaveCapabilities`. The crate docs explain the limits of clock stretching.

- `I2cMonitor::records` lists its errors with `#[errors(..)]` (API-32), now that stream functions and properties can
  carry one, instead of a hand-written `# Errors` note: rustdoc, introspection and the client's warning about an
  unlisted error see it.
- `I2c::i2c_transaction`: several reads and writes to one address with repeated starts (embedded-hal style).
- `I2c::i2c_lines_state`, `i2c_wiggle_clock` (free a stuck bus), `i2c_set_pullups`, `i2c_pullups`.
- `I2cMonitor` trait: `records(io_level)`, a counted stream function (API-31) with one `I2cRecord` per transaction;
  the start condition's time and dropped records are in the event head.
- `I2cWatch::changes` is a counted stream (API-31): each result's read time and dropped results are in the event
  head, `I2cWatchEvent` holds only the slot and the result.
- Crate and method docs, with an `# Errors` section per method.
- End-to-end test pair `tests/io_i2c` + `tests/io_i2c_api` (STD-13 fill-in): a mock server on `ww_device::Server`
  exercising `I2c`, `I2cWatch` (all three `I2cWatchReport` rules plus the error-once-then-recovery sequence) and
  `I2cMonitor`, and a unit test checking `I2cAddress::seven`/`::ten` wire sizes (1 and 2 bytes). Docs page
  `docs/std_library/ww_i2c.md`.

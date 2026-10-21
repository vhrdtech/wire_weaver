## Unreleased

Draft of the IO trait family (STD-13), under review; not published yet, so no version bump.

### ⚠️ Breaking

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

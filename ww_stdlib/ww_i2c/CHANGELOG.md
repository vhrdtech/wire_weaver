## Unreleased

Draft of the IO trait family (STD-13), under review; not published yet, so no version bump.

### ⚠️ Breaking

- Addresses are `I2cAddress { Seven(u8), Ten(u16) }` instead of `u8` (10-bit addressing; `From<u8>` gives the
  7-bit one).
- `i2c_configure` takes `io_level: Option<Volt>` instead of `io_level_mv: u16` (`None` leaves the level as it is).
- `i2c_write` data and `i2c_scan` results are `MaxLen<RefVec<'i, u8>, N>` instead of `Vec<u8>` (`MAX_WRITE_LEN`,
  `MAX_SCAN`), so a `no_alloc` device can serve them; `I2cReadKind::RepeatedStart::write` is bounded too.
- `delay_after` is `Option<Second>` instead of a `u32` without a unit.
- `RawTimestamp` is gone: `I2cTransactionTimeRange` holds `ww_si::Second` device time.
- `I2cError` gains `InUse`, `NoIoLevel`, `Refused`, `Unsupported`.
- `I2cCapabilities` lists IO levels as `Volt`s, bounds `driver_name`, adds `ten_bit_addresses`, `pullups`,
  `bus_recovery`.

### 🚀 Features

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

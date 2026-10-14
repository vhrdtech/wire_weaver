## Unreleased

Draft of the IO trait family (STD-13), under review; not published yet, so no version bump.

### ⚠️ Breaking

- `i2c_configure` takes `io_level: Option<Volt>` instead of `io_level_mv: u16` (`None` leaves the level as it is).
- `i2c_write` data and `i2c_scan` results are `MaxLen<RefVec<'i, u8>, N>` instead of `Vec<u8>` (`MAX_WRITE_LEN`,
  `MAX_SCAN`), so a `no_alloc` device can serve them; `I2cReadKind::RepeatedStart::write` is bounded too.
- `delay_after` is `Option<Second>` instead of a `u32` without a unit.
- `RawTimestamp` is gone: `I2cTransactionTimeRange` holds `ww_si::Second` device time.
- `I2cError` is `Unsized` and borrows (`I2cError<'i>`); new variants `InUse { by }`, `NoIoLevel`,
  `Refused { reason }`, `Unsupported`, and `#[future_version] Unknown(u32)`.
- `I2cCapabilities` lists IO levels as `Volt`s and gains `timestamps`; `driver_name` is bounded.

### 🚀 Features

- `I2c::i2c_transaction`: several reads and writes to one address with repeated starts (embedded-hal style).
- `I2cBus` trait: line levels, `wiggle_clock` to free a stuck bus, switchable pull-ups.
- `I2cMonitor` trait: a `records` stream with one `I2cRecord` per transaction (`seq`, `lost`, `t` head).
- Crate and method docs, with an `# Errors` section per method.

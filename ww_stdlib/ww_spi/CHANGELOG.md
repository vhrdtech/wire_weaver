## Unreleased

### ⚠️ Breaking

- `SpiRole::Slave` and `SpiCapabilities::slave` are removed (STD-35): slave mode is the new `slave` group of `Spi`
  (below), opened with `Spi::slave_frames`; whether a device has it is in its introspection and
  `Spi::slave_capabilities`.

- `SpiError<'i>`: the first variant is `#[common] Common(CommonError<'i>)` (STD-16): `Unsupported`, `InvalidArgument`,
  `InUse`, `Busy`, `WrongState`, `NoPower`, `Refused`, `Timeout` come from `ww_error::CommonError`; `BufferTooBig`
  stays. It is no longer `self_describing` (newer variants read as `Unknown(n)`), carries a lifetime (`SpiErrorOwned` on
  hosts), has `Display` from its doc lines, and every method lists its variants with `#[errors(..)]`. Devices return
  `CommonError::Unsupported.into()` (or `?` a helper returning `CommonError`).
- `SpiMonitor` is gone: `SpiMonitor::frames` is `Spi::monitor`, an `#[optional]` counted stream function of `Spi`
  (STD-13), so an array of buses carries monitoring without a second trait. An adapter without monitoring hardware
  leaves it out with `omit = ["spi.monitor"]`; `SpiCapabilities::monitor` is removed (introspection says whether the
  item is there).
- The IO level is `SpiConfig::io_level: Option<Volt>` instead of the `reference_voltage` property, and
  `Spi::configure` answers the actual `SpiConfig` instead of `SpiActual` (removed) (STD-13). `SpiMonitorConfig` gets
  `io_level`; `SpiCapabilities::io_levels` is bounded by `MAX_IO_LEVELS` and gets `io_level_min` / `io_level_max`.

### 🚀 Features

- `Spi` is `#[claim(exclusive)]` (SRV-34): a client can claim a controller through the hub. Wire compatible, only the introspection data changes.
- Slave mode (STD-35): an `#[optional(slave)]` group in `Spi` (`omit = ["spi.slave"]` without it).
  `slave_frames(SpiSlaveConfig)` is a counted stream function: opening it makes the device a slave, and it reports
  every frame as a `SpiSlaveFrame` (MOSI, MISO, address, `underrun`, `overrun`). Answers come from the
  `slave_answers` credit sink, one per frame (`SpiSlaveAnswers::Queue { idle, repeat_last }`, cleared with
  `slave_clear_answers`), or from a register map (`SpiSlaveAnswers::Registers(SpiRegisters)`: address bytes, a
  `SpiRwBit`, dummy bytes; filled with `slave_map_write`, read with `slave_map_read`); `slave_capabilities` answers
  `SpiSlaveCapabilities`.

- `Spi::monitor` lists its errors with `#[errors(..)]` (API-32), now that stream functions and properties can carry one,
  instead of a hand-written `# Errors` note: rustdoc, introspection and the client's warning about an unlisted error see
  it.
- The empty `Spi` trait gets its API (STD-13, skeleton): `transfer`, `transaction` (CS held), `write`, `configure` returning the frequency actually set, manual CS, `reference_voltage`; `SpiMonitor` with `frames(config)`, a counted stream function (API-31), one `SpiFrame` per CS assertion (when CS was asserted is the time in the event head).
- Full rustdoc on every trait, method and type, with an `# Errors` section per fallible method (STD-13).

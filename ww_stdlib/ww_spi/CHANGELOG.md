## Unreleased

### ⚠️ Breaking

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

- The empty `Spi` trait gets its API (STD-13, skeleton): `transfer`, `transaction` (CS held), `write`, `configure` returning the frequency actually set, manual CS, `reference_voltage`; `SpiMonitor` with `frames(config)`, a counted stream function (API-31), one `SpiFrame` per CS assertion (when CS was asserted is the time in the event head).
- Full rustdoc on every trait, method and type, with an `# Errors` section per fallible method (STD-13).

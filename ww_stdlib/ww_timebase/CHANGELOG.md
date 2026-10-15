## Unreleased

### ⚠️ Breaking

- `ClockError<'i>`: its only variant so far is the `#[common] Common(CommonError<'i>)` (STD-16): `Unsupported`,
  `InvalidArgument`, `InUse`, `Busy`, `WrongState`, `NoPower`, `Refused`, `Timeout` come from `ww_error::CommonError`.
  It is no longer `self_describing` (newer variants read as `Unknown(n)`), carries a lifetime (`ClockErrorOwned` on
  hosts), has `Display` from its doc lines, and every method lists its variants with `#[errors(..)]`. Devices return
  `CommonError::Unsupported.into()` (or `?` a helper returning `CommonError`); stream-function handlers return
  `ClockError<'static>` for now (API-35).
### 🚀 Features

- New crate (STD-17): `Clock` with `now`, `info`, sync master / slave (IRIG-B, PPS), `sync_status`, a `time_map` stream mapping device time to shared time, `set_time_of_day` for a master. Timestamps are `ww_si::Second`, no own time type. Full rustdoc with `# Errors` on `configure_sync` / `set_time_of_day`, docs page (`docs/std_library/ww_timebase.md`), a mock server and in-process tests (`tests/io_timebase`).

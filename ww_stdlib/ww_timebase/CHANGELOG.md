## Unreleased

### ⚠️ Breaking

- `ClockError<'i>`: its only variant so far is the `#[common] Common(CommonError<'i>)` (STD-16): `Unsupported`,
  `InvalidArgument`, `InUse`, `Busy`, `WrongState`, `NoPower`, `Refused`, `Timeout` come from `ww_error::CommonError`.
  It is no longer `self_describing` (newer variants read as `Unknown(n)`), carries a lifetime (`ClockErrorOwned` on
  hosts), has `Display` from its doc lines, and every method lists its variants with `#[errors(..)]`. Devices return
  `CommonError::Unsupported.into()` (or `?` a helper returning `CommonError`).
### 🚀 Features

- Host sync (STD-49): `SyncFormat::Host` (a slave format: a host steers the device over the API, no sync line) and
  the optional group `host_sync` of `Clock`: `steer(point: SteerPoint)` (errors `Common(WrongState)` unless
  configured as a `Host` slave, `Common(InvalidArgument)` for a point the device can't use) and
  `shared_now() -> Option<Second>`. `SteerPoint { local, shared, rate_ppb }` is a host's fit of the device's clock,
  in the prefix and storage of `now()`. Servers that don't steer leave the group out with
  `omit = ["clock.host_sync"]` (or the path of their `Clock` item); servers built before this need that line or the
  two handlers (`ww api scaffold` prints them).
- `ww_timebase::host` (feature `std`): `Estimator` (min-RTT `now()` samples, least-squares offset and rate over a
  sliding window, `Fit` with `shared_at` / `local_at` / `rate_ppb`) and `DeviceTime` (device timestamps to
  nanoseconds and back, unwrapping a `U32` `now()`, `steer_point` in the device's own prefix and storage). Takes
  time as numbers, never reads a clock: tested with simulated device clocks (1 % fast HSI, a drifting crystal, USB
  full-speed jitter), and end to end against the mock in `tests/io_timebase`.

- New crate (STD-17): `Clock` with `now`, `info`, sync master / slave (IRIG-B, PPS), `sync_status`, a `time_map` stream mapping device time to shared time, `set_time_of_day` for a master. Timestamps are `ww_si::Second`, no own time type. Full rustdoc with `# Errors` on `configure_sync` / `set_time_of_day`, docs page (`docs/std_library/ww_timebase.md`), a mock server and in-process tests (`tests/io_timebase`).

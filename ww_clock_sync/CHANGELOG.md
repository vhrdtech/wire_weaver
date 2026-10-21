## Unreleased

### ⚠️ Breaking

- `Options` is `Clone`, no longer `Copy` (it holds a `SteererOwned` and a `RateCache` now); `Status::device` is a
  `SyncStatusOwned`; `Fit` has a `prior` field; `EstimatorConfig` a `prior_span` one (SRV-31).

### 🚀 Features

- Rate cache (SRV-31): `RateCache::for_device(serial)` keeps each device's clock rate against this PC in the user's
  state dir (`state_dir()`: `$XDG_STATE_HOME/ww`, `~/Library/Application Support/ww`, `%LOCALAPPDATA%\ww\state`,
  or `WW_STATE_DIR`), one file per serial, written to a temporary file and renamed. `Options::rate_cache` reads it at
  the start and saves the fit's rate every `Options::save_rate_every` (60 s); `Estimator::set_prior` fits the
  offset alone with the known rate from the first sample on, until the samples cover `EstimatorConfig::prior_span`
  (20 s), so the first steer point goes out a round trip after connecting. `Status::rate_from_cache`, `Fit::prior`.
- Steerer (SRV-31): every steer point names its sender (`Options::steerer`, default `steerer(None)`: program, host,
  pid and a random id); `Status::followed` says whether the device follows this sync or another steerer
  (`Status::device.steerer` names it).
- `ClockSync::set_paused`: reads go on, steer points stop, the device coasts in holdover (the hub pauses under a
  device-wide claim, SRV-32); `Status::paused`.

- Faster first lock (STD-49): `Options::acquire` (`Pace`: poll, steer and check intervals) is used until the device
  says `Locked`: by default a reading and a steer point every 100 ms and a check every second, then the normal
  200 ms / 1 s / 5 s. `Options::pace(state)` picks it, `Status::acquiring` says which one runs. With bedrock_sync's
  5 % slew while acquiring, a board 250 ms off is within 2 ms in about 5 s (was 25 s at 1 % and 1 Hz).
- New crate (STD-49): host clock sync for any process holding a device connection. `ClockSync::start(&Commander,
  Options)` reads the device's `ww_timebase::Clock::now()` a few times a second, fits it against the host's
  monotonic clock (`Estimator`: min-RTT samples, least squares over a sliding window) and steers it onto UTC with
  `Clock::steer` once a second (`DeviceTime`: steer points in the device's own prefix and storage; `HostClock`: the
  wall clock's offset added when sending, so a step of it doesn't disturb the fit). `Status` has the fit, the source
  the device follows and the residual offset measured with `Clock::shared_now`.

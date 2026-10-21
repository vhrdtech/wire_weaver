## Unreleased

### 🚀 Features

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

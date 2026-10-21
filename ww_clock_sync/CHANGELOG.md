## Unreleased

### 🚀 Features

- New crate (STD-49): host clock sync for any process holding a device connection. `ClockSync::start(&Commander,
  Options)` reads the device's `ww_timebase::Clock::now()` a few times a second, fits it against the host's
  monotonic clock (`Estimator`: min-RTT samples, least squares over a sliding window) and steers it onto UTC with
  `Clock::steer` once a second (`DeviceTime`: steer points in the device's own prefix and storage; `HostClock`: the
  wall clock's offset added when sending, so a step of it doesn't disturb the fit). `Status` has the fit, the source
  the device follows and the residual offset measured with `Clock::shared_now`.

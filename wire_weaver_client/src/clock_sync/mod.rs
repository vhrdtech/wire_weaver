//! Host clock sync for WireWeaver devices (STD-49): every device on UTC, Klipper-style.
//!
//! A device serving `ww_timebase::Clock` with the `host_sync` group keeps a shared clock steered by its host. This
//! module is the host side, for any process holding a device connection: every direct connection steers its device
//! by itself ([ClockSteering], CLT-46, from [Commander::clock_sync](crate::Commander::clock_sync)), the ww hub the
//! devices it holds (SRV-31), and [ClockSync] runs on any `Commander` for a program that steers with options of its
//! own (`ww sync`). Every one of them steers onto the same timescale, the host's wall clock (UTC),
//! so devices agree without a central daemon, and devices steered by different PCs agree to those PCs' NTP.
//!
//! - [Estimator] fits the device's clock as a line over the host's monotonic clock, from `now()` readings with the
//!   shortest round trips (Klipper's clock sync).
//! - [DeviceTime] turns device timestamps into nanoseconds and back into the device's own prefix and storage, and
//!   builds the `SteerPoint` of a fit.
//! - [HostClock] reads the monotonic clock and the wall clock's offset from it.
//! - [ClockSync] runs all of it for one device in a tokio task.
//!
//! The estimator and the conversions take time as numbers, never read a clock or sleep: they are tested with
//! simulated device clocks (an HSI 1 % fast, a drifting crystal, USB full-speed jitter). `docs/std_library/
//! ww_timebase.md` explains the model: sources ranked line > host > holdover > free, slewing, what to expect.

pub(crate) mod auto;
mod device_time;
mod estimator;
mod host_clock;
mod rate_cache;
mod sync;

pub use auto::{ClockSteering, ENV, Phase, rate_key, serves_host_sync};
pub(crate) use auto::{Start, enabled_from_env, prepare, start};
pub use device_time::{DeviceTime, DeviceTimeError};
pub use estimator::{Added, Estimator, EstimatorConfig, Fit, Sample};
pub use host_clock::HostClock;
pub use rate_cache::{CachedRate, RateCache, STATE_DIR_ENV, state_dir};
pub use sync::{ClockClient, ClockSync, Options, Pace, Status, steerer};

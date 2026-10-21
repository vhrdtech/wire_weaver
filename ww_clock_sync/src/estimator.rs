//! The fit: a device's clock as a line over host monotonic time, from `Clock::now()` samples (Klipper's clock sync).
//!
//! [Estimator] keeps the samples with the shortest round trips (their midpoint is closest to when the device read
//! its clock) and fits a line over a sliding window: host time as a function of device time, an offset and a rate.
//! It takes time as numbers (nanoseconds), never reads a clock or sleeps, so tests drive it with simulated devices.
//!
//! ```
//! use ww_clock_sync::{Estimator, EstimatorConfig, Sample};
//!
//! // a device whose clock runs 1 % fast and started 5 s before the host's clock
//! let device = |host_ns: u64| ((host_ns + 5_000_000_000) as f64 * 1.01) as u64;
//! let mut estimator = Estimator::new(EstimatorConfig::default());
//! for i in 0..20u64 {
//!     let sent = i * 200_000_000;
//!     // 300 µs there, 300 µs back
//!     let sample = Sample { sent, device: device(sent + 300_000), received: sent + 600_000 };
//!     estimator.add(sample);
//! }
//! let fit = estimator.fit().unwrap();
//! assert!((fit.rate_ppb() + 9_900_990).abs() < 100);
//! let local = device(3_000_000_000);
//! assert!((fit.shared_at(local) - 3_000_000_000.0).abs() < 1_000.0);
//! ```

use std::collections::VecDeque;
use std::time::Duration;

/// One reading of a device's clock: host time when the request left and when the answer came back, and the
/// device's time in between, all in nanoseconds (the device's after [DeviceTime::to_ns](crate::DeviceTime::to_ns)).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Sample {
    /// Host monotonic time the `now()` request was sent at.
    pub sent: u64,
    /// Device time it answered with, unwrapped.
    pub device: u64,
    /// Host monotonic time the answer came back at.
    pub received: u64,
}

impl Sample {
    /// Round trip.
    pub fn rtt(&self) -> u64 {
        self.received.saturating_sub(self.sent)
    }

    /// Host time the device most likely read its clock at: the middle of the round trip.
    pub fn host_mid(&self) -> u64 {
        self.sent + self.rtt() / 2
    }
}

/// How [Estimator] picks and fits samples.
#[derive(Copy, Clone, Debug)]
pub struct EstimatorConfig {
    /// Samples older than this (host time, from the newest) are dropped: long enough to average the jitter and
    /// see the rate well, short enough to follow a clock that drifts with temperature.
    pub window: Duration,
    /// Most samples kept, the oldest go first.
    pub max_samples: usize,
    /// Share of the window's samples, the ones with the shortest round trips, the line is fitted to.
    pub keep_fraction: f64,
    /// Fewest samples (after the filter) a fit needs.
    pub min_samples: usize,
    /// Shortest span of device time the samples must cover before the rate is trusted.
    pub min_span: Duration,
    /// With a rate known from before ([Estimator::set_prior], the rate cache): the samples must cover this span of
    /// device time before their own rate replaces it. Until then the fit takes the known rate and fits only the
    /// offset, from the first sample on: a young fit's rate is off by the jitter over its span (100 µs over 1 s is
    /// 100 ppm), a cached one by what the clock drifted since.
    pub prior_span: Duration,
}

impl Default for EstimatorConfig {
    /// 30 s window, up to 512 samples, the best half fitted, from 4 samples over 1 s on; a known rate is kept for
    /// the first 20 s (10 s of USB full-speed jitter pin the samples' own rate to tens of ppm only).
    fn default() -> Self {
        EstimatorConfig {
            window: Duration::from_secs(30),
            max_samples: 512,
            keep_fraction: 0.5,
            min_samples: 4,
            min_span: Duration::from_secs(1),
            prior_span: Duration::from_secs(20),
        }
    }
}

/// What [Estimator::add] did with a sample.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Added {
    /// Kept for the next fit.
    Kept,
    /// The device's time went backwards (it restarted): the earlier samples were dropped, this one starts over.
    Restarted,
    /// Received before it was sent: not kept.
    Invalid,
}

/// Fits device time to host time from [Sample]s, see the [crate docs](crate).
#[derive(Clone, Debug)]
pub struct Estimator {
    config: EstimatorConfig,
    samples: VecDeque<Sample>,
    /// Slope known from before, see [Estimator::set_prior]
    prior: Option<f64>,
}

impl Estimator {
    pub fn new(config: EstimatorConfig) -> Self {
        Estimator {
            config,
            samples: VecDeque::new(),
            prior: None,
        }
    }

    /// A slope known from before (host ns per device ns, [Fit::slope] of an earlier fit of the same device, e.g. from
    /// the [RateCache](crate::RateCache)): fits use it from the first sample on, until the samples cover
    /// [EstimatorConfig::prior_span]. `None` forgets it.
    pub fn set_prior(&mut self, slope: Option<f64>) {
        self.prior = slope.filter(|s| s.is_finite() && *s > 0.0);
    }

    /// Adds a sample, in host time order.
    pub fn add(&mut self, sample: Sample) -> Added {
        if sample.received < sample.sent {
            return Added::Invalid;
        }
        let restarted = self
            .samples
            .back()
            .is_some_and(|last| sample.device < last.device);
        if restarted {
            self.samples.clear();
        }
        self.samples.push_back(sample);
        let window = self.config.window.as_nanos() as u64;
        while let Some(first) = self.samples.front() {
            let too_old = sample.received.saturating_sub(first.received) > window;
            if too_old || self.samples.len() > self.config.max_samples.max(2) {
                self.samples.pop_front();
            } else {
                break;
            }
        }
        if restarted {
            Added::Restarted
        } else {
            Added::Kept
        }
    }

    /// Drops every sample (the device reconnected or its clock was set).
    pub fn clear(&mut self) {
        self.samples.clear();
    }

    /// Samples in the window.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Device time between the window's oldest and newest sample, in ns.
    pub fn span(&self) -> u64 {
        let (min, max) = self.samples.iter().fold((u64::MAX, 0), |(lo, hi), s| {
            (lo.min(s.device), hi.max(s.device))
        });
        max.saturating_sub(min)
    }

    /// Shortest round trip in the window.
    pub fn min_rtt(&self) -> Option<u64> {
        self.samples.iter().map(Sample::rtt).min()
    }

    /// The line through the window's best samples, once there are enough of them over a long enough span; with a
    /// [prior](Self::set_prior) slope, the line of that slope through them from the first sample on, until they
    /// cover [EstimatorConfig::prior_span].
    pub fn fit(&self) -> Option<Fit> {
        let mut best: Vec<&Sample> = self.samples.iter().collect();
        best.sort_by_key(|s| (s.rtt(), s.sent));
        let keep = ((best.len() as f64 * self.config.keep_fraction).ceil() as usize)
            .max(self.config.min_samples)
            .min(best.len());
        best.truncate(keep);
        let (min, max) = best.iter().fold((u64::MAX, 0), |(lo, hi), s| {
            (lo.min(s.device), hi.max(s.device))
        });
        if let Some(slope) = self.prior
            && !best.is_empty()
            && max - min < self.config.prior_span.as_nanos() as u64
        {
            return Some(fixed_slope(&best, max, slope));
        }
        if best.len() < self.config.min_samples.max(2) {
            return None;
        }
        if max - min < self.config.min_span.as_nanos() as u64 {
            return None;
        }
        // least squares of host time over device time, centred on the newest device time for precision
        let x0 = max;
        let y0 = best.iter().map(|s| s.host_mid()).max().unwrap_or(0);
        let n = best.len() as f64;
        let points = best
            .iter()
            .map(|s| (s.device as f64 - x0 as f64, s.host_mid() as f64 - y0 as f64));
        let (sx, sy) = points
            .clone()
            .fold((0.0, 0.0), |(sx, sy), (x, y)| (sx + x, sy + y));
        let (mx, my) = (sx / n, sy / n);
        let (sxx, sxy) = points.clone().fold((0.0, 0.0), |(sxx, sxy), (x, y)| {
            (sxx + (x - mx) * (x - mx), sxy + (x - mx) * (y - my))
        });
        if sxx <= 0.0 {
            return None;
        }
        let slope = sxy / sxx;
        let intercept = my - slope * mx;
        let residual = (points
            .map(|(x, y)| {
                let e = y - (intercept + slope * x);
                e * e
            })
            .sum::<f64>()
            / n)
            .sqrt();
        Some(Fit {
            local: x0,
            shared: y0 as f64 + intercept,
            slope,
            min_rtt: best.iter().map(|s| s.rtt()).min().unwrap_or(0),
            used: best.len(),
            residual_rms: residual,
            prior: false,
        })
    }
}

/// The line of a known `slope` through `best`, at device time `x0`: only the offset is fitted.
fn fixed_slope(best: &[&Sample], x0: u64, slope: f64) -> Fit {
    let y0 = best.iter().map(|s| s.host_mid()).max().unwrap_or(0);
    let n = best.len() as f64;
    let offsets = best
        .iter()
        .map(|s| s.host_mid() as f64 - y0 as f64 - slope * (s.device as f64 - x0 as f64));
    let mean = offsets.clone().sum::<f64>() / n;
    let residual = (offsets.map(|o| (o - mean) * (o - mean)).sum::<f64>() / n).sqrt();
    Fit {
        local: x0,
        shared: y0 as f64 + mean,
        slope,
        min_rtt: best.iter().map(|s| s.rtt()).min().unwrap_or(0),
        used: best.len(),
        residual_rms: residual,
        prior: true,
    }
}

/// A device's clock as a line: host (shared) time over device (local) time, in nanoseconds.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Fit {
    /// Device time of the reference point (the newest sample's).
    pub local: u64,
    /// Host time at `local`.
    pub shared: f64,
    /// Host nanoseconds per device nanosecond.
    pub slope: f64,
    /// Shortest round trip of the samples used.
    pub min_rtt: u64,
    /// Samples the line was fitted to.
    pub used: usize,
    /// RMS distance of those samples from the line, in nanoseconds: the jitter left after the filter.
    pub residual_rms: f64,
    /// The slope is the [prior](Estimator::set_prior) one, not the samples' own yet.
    pub prior: bool,
}

impl Fit {
    /// Host time at device time `local`.
    pub fn shared_at(&self, local: u64) -> f64 {
        self.shared + (local as f64 - self.local as f64) * self.slope
    }

    /// Device time at host time `shared`.
    pub fn local_at(&self, shared: f64) -> f64 {
        self.local as f64 + (shared - self.shared) / self.slope
    }

    /// The fit's own error estimate in µs, as `SteerPoint::error_us` has it: half the shortest round trip (where in
    /// it the device read its clock is unknown) plus twice the scatter of the samples, rounded up.
    pub fn error_us(&self) -> u32 {
        ((self.min_rtt as f64 / 2.0 + 2.0 * self.residual_rms) / 1000.0)
            .ceil()
            .min(u32::MAX as f64) as u32
    }

    /// Shared rate minus local rate in parts per billion, as `SteerPoint::rate_ppb` has it.
    pub fn rate_ppb(&self) -> i32 {
        ((self.slope - 1.0) * 1e9)
            .round()
            .clamp(i32::MIN as f64, i32::MAX as f64) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A simulated device clock: `rate` device ns per host ns, starting at `start` device ns at host time 0, plus a
    /// slow wander of `wander` ppb per second of host time (temperature).
    #[derive(Copy, Clone)]
    struct SimClock {
        start: f64,
        rate: f64,
        wander: f64,
    }

    impl SimClock {
        fn at(&self, host: u64) -> u64 {
            let t = host as f64;
            let rate = self.rate + self.wander * 1e-9 * t / 1e9;
            (self.start + t * rate) as u64
        }

        /// Host time when this device reads `local`, by bisection.
        fn host_at(&self, local: u64) -> f64 {
            let (mut lo, mut hi) = (0u64, 1u64 << 50);
            while hi - lo > 1 {
                let mid = (lo + hi) / 2;
                if self.at(mid) < local {
                    lo = mid
                } else {
                    hi = mid
                }
            }
            hi as f64
        }
    }

    /// Deterministic jitter: xorshift.
    struct Jitter(u64);

    impl Jitter {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        /// 0..max
        fn below(&mut self, max: u64) -> u64 {
            self.next() % max
        }
    }

    /// Polls `clock` every `poll` ns for `duration` ns of host time like a USB FS link: each way waits for the
    /// next 1 ms frame (0..1 ms) plus up to `extra` ns, and one sample in ten is slow (a busy host, 2..5 ms).
    fn run(clock: SimClock, duration: u64, poll: u64, extra: u64, seed: u64) -> (Estimator, u64) {
        run_with(
            Estimator::new(EstimatorConfig::default()),
            clock,
            duration,
            poll,
            extra,
            seed,
        )
    }

    fn run_with(
        mut e: Estimator,
        clock: SimClock,
        duration: u64,
        poll: u64,
        extra: u64,
        seed: u64,
    ) -> (Estimator, u64) {
        let mut j = Jitter(seed);
        let mut t = 0;
        while t < duration {
            let there = 50_000 + j.below(1_000_000) + j.below(extra.max(1));
            let mut back = 50_000 + j.below(1_000_000) + j.below(extra.max(1));
            if j.below(10) == 0 {
                back += 2_000_000 + j.below(3_000_000);
            }
            let s = Sample {
                sent: t,
                device: clock.at(t + there),
                received: t + there + back,
            };
            assert_eq!(e.add(s), Added::Kept);
            t += poll;
        }
        (e, t)
    }

    #[test]
    fn hsi_one_percent_fast_over_usb_fs() {
        let clock = SimClock {
            start: 7e9,
            rate: 1.01,
            wander: 0.0,
        };
        let (e, end) = run(clock, 20_000_000_000, 200_000_000, 0, 1);
        let fit = e.fit().unwrap();
        // rate to a few ppm
        let want = (1.0 / 1.01 - 1.0) * 1e9;
        assert!(
            (fit.rate_ppb() as f64 - want).abs() < 5_000.0,
            "{}",
            fit.rate_ppb()
        );
        // offset now, and 10 s ahead (holdover on the fit), to well under a ms
        for host in [end, end + 10_000_000_000] {
            let local = clock.at(host);
            let err = fit.shared_at(local) - clock.host_at(local);
            assert!(err.abs() < 300_000.0, "at {host}: {err} ns");
        }
        // the best half of the samples: the slow ones are out
        assert!(fit.min_rtt < 1_000_000);
        assert!(fit.residual_rms < 400_000.0, "{}", fit.residual_rms);
    }

    #[test]
    fn crystal_and_wander_follow() {
        // 30 ppm slow, drifting 0.2 ppm per second, polled for 2 minutes
        let clock = SimClock {
            start: 0.0,
            rate: 1.0 - 30e-6,
            wander: 200.0,
        };
        let (e, end) = run(clock, 120_000_000_000, 200_000_000, 200_000, 7);
        let fit = e.fit().unwrap();
        let local = clock.at(end);
        let err = fit.shared_at(local) - clock.host_at(local);
        assert!(err.abs() < 500_000.0, "{err} ns");
        // the window holds the last 30 s only
        assert!(e.len() <= 151, "{}", e.len());
    }

    #[test]
    fn many_seeds_stay_within_bounds() {
        for seed in 1..50 {
            let clock = SimClock {
                start: 1e9 * seed as f64,
                rate: 1.0 + (seed as f64 - 25.0) * 4e-4,
                wander: 0.0,
            };
            let (e, end) = run(clock, 10_000_000_000, 100_000_000, 100_000, seed);
            let fit = e.fit().unwrap();
            let local = clock.at(end);
            let err = fit.shared_at(local) - clock.host_at(local);
            assert!(err.abs() < 400_000.0, "seed {seed}: {err} ns");
            let want = (1.0 / clock.rate - 1.0) * 1e9;
            // 10 s of FS jitter (up to 2 ms a round trip) pins the rate to tens of ppm; 20 s and more to a few
            assert!(
                (fit.rate_ppb() as f64 - want).abs() < 50_000.0,
                "seed {seed}: {} vs {want}, err {err}",
                fit.rate_ppb()
            );
        }
    }

    #[test]
    fn no_fit_until_enough_span() {
        let clock = SimClock {
            start: 0.0,
            rate: 1.0,
            wander: 0.0,
        };
        let (e, _) = run(clock, 800_000_000, 100_000_000, 0, 3);
        assert!(e.fit().is_none(), "8 samples over 0.7 s");
        let (e, _) = run(clock, 1_400_000_000, 100_000_000, 0, 3);
        assert!(e.fit().is_some());
    }

    #[test]
    fn a_known_rate_fits_from_the_first_sample() {
        let clock = SimClock {
            start: 3e9,
            rate: 1.01,
            wander: 0.0,
        };
        // the cache's rate, 2 ppm off what the clock does today
        let slope = 1.0 / 1.01 + 2e-6;
        let mut e = Estimator::new(EstimatorConfig::default());
        e.set_prior(Some(slope));
        let (e, end) = run_with(e, clock, 1, 100_000_000, 0, 5);
        let fit = e.fit().expect("one sample is enough");
        assert!(fit.prior);
        let local = clock.at(end);
        // within the sample's round trip (up to ~2.1 ms on FS)
        let err = fit.shared_at(local) - clock.host_at(local);
        assert!(err.abs() < 2_100_000.0, "{err} ns");
        // 5 s later: the offset from the best of 50 samples, the rate still the cache's
        let mut e2 = Estimator::new(EstimatorConfig::default());
        e2.set_prior(Some(slope));
        let (e2, end) = run_with(e2, clock, 5_000_000_000, 100_000_000, 0, 5);
        let fit = e2.fit().unwrap();
        assert!(fit.prior && fit.slope == slope);
        let local = clock.at(end);
        let err = fit.shared_at(local) - clock.host_at(local);
        assert!(err.abs() < 600_000.0, "{err} ns");
        // past the prior span the samples' own rate takes over
        let mut e3 = Estimator::new(EstimatorConfig::default());
        e3.set_prior(Some(slope));
        let (e3, _) = run_with(e3, clock, 25_000_000_000, 200_000_000, 0, 5);
        assert!(!e3.fit().unwrap().prior);
    }

    #[test]
    fn restart_and_invalid_samples() {
        let mut e = Estimator::new(EstimatorConfig::default());
        let s = |sent: u64, device: u64| Sample {
            sent,
            device,
            received: sent + 1000,
        };
        assert_eq!(e.add(s(0, 5_000)), Added::Kept);
        assert_eq!(e.add(s(10, 6_000)), Added::Kept);
        assert_eq!(e.add(s(20, 100)), Added::Restarted);
        assert_eq!(e.len(), 1);
        let bad = Sample {
            sent: 30,
            device: 200,
            received: 29,
        };
        assert_eq!(e.add(bad), Added::Invalid);
        assert_eq!(e.len(), 1);
    }
}

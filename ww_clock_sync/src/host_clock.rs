//! The host's clocks: monotonic for the fit, the wall clock (UTC) for what is sent.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// The host's monotonic clock in nanoseconds since this value was made, and the wall clock's offset from it.
///
/// Fits run on the monotonic clock, so a step of the wall clock (NTP setting it, someone changing the date) doesn't
/// throw away the samples; the offset to UTC is read again for every steer point.
#[derive(Copy, Clone, Debug)]
pub struct HostClock {
    anchor: Instant,
}

impl HostClock {
    pub fn new() -> Self {
        HostClock {
            anchor: Instant::now(),
        }
    }

    /// Monotonic nanoseconds.
    pub fn monotonic_ns(&self) -> u64 {
        self.anchor.elapsed().as_nanos() as u64
    }

    /// UTC minus monotonic time, in nanoseconds: UTC of a monotonic time `m` is `m + utc_offset()`. Read as the
    /// tightest of a few monotonic / wall / monotonic triples.
    pub fn utc_offset(&self) -> i128 {
        (0..3)
            .map(|_| {
                let before = self.monotonic_ns();
                let wall = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_nanos() as i128)
                    .unwrap_or(0);
                let after = self.monotonic_ns();
                (
                    after - before,
                    wall - (before + (after - before) / 2) as i128,
                )
            })
            .min_by_key(|(width, _)| *width)
            .map(|(_, offset)| offset)
            .unwrap_or(0)
    }

    /// UTC now, in nanoseconds since the Unix epoch.
    pub fn utc_ns(&self) -> i128 {
        self.monotonic_ns() as i128 + self.utc_offset()
    }
}

impl Default for HostClock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_matches_the_wall_clock() {
        let clock = HostClock::new();
        let wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as i128;
        // the same moment within a second, whatever the load
        assert!((clock.utc_ns() - wall).abs() < 1_000_000_000);
        let a = clock.monotonic_ns();
        assert!(clock.monotonic_ns() >= a);
    }
}

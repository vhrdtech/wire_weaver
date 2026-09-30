use core::ops::Add;
use core::time::Duration;

/// Monotonic time point in microseconds since an arbitrary epoch (e.g., boot), provided by the caller.
///
/// Kept independent of any runtime, so that the core can be driven by embassy, RTIC, a bare-metal
/// timer or a test with hand-advanced time alike.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Instant(u64);

impl Instant {
    pub const fn from_micros(us: u64) -> Self {
        Instant(us)
    }

    pub const fn from_millis(ms: u64) -> Self {
        Instant(ms * 1000)
    }

    pub const fn as_micros(self) -> u64 {
        self.0
    }

    pub fn saturating_duration_since(self, earlier: Instant) -> Duration {
        Duration::from_micros(self.0.saturating_sub(earlier.0))
    }
}

impl Add<Duration> for Instant {
    type Output = Instant;

    fn add(self, rhs: Duration) -> Instant {
        Instant(self.0.saturating_add(rhs.as_micros() as u64))
    }
}

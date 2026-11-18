//! Settings of a keep-trying connection (CLT-75), see
//! [PreparedConnection::connect_keep_trying](crate::PreparedConnection::connect_keep_trying).

use std::time::Duration;

/// The wait between connection attempts: `first` after the first failed one, doubled after each further one up to
/// `max`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Backoff {
    pub first: Duration,
    pub max: Duration,
}

impl Backoff {
    /// Between reconnects through the ww hub (CLT-40): the hub runs on the same PC and is back within milliseconds.
    pub(crate) const HUB: Backoff = Backoff {
        first: Duration::from_millis(10),
        max: Duration::from_secs(1),
    };

    /// [KeepTrying]'s default: a networked device that boots, or a Wi-Fi link that comes back, within seconds.
    pub const NETWORK: Backoff = Backoff {
        first: Duration::from_millis(100),
        max: Duration::from_secs(5),
    };

    /// The wait after the one of `current`.
    pub(crate) fn after(&self, current: Duration) -> Duration {
        current.saturating_mul(2).min(self.max)
    }

    pub(crate) fn check(&self) -> anyhow::Result<()> {
        if self.first.is_zero() {
            anyhow::bail!("Backoff::first must not be zero");
        }
        if self.max < self.first {
            anyhow::bail!(
                "Backoff::max ({:?}) must not be below Backoff::first ({:?})",
                self.max,
                self.first
            );
        }
        Ok(())
    }
}

/// How a keep-trying connection tries, see [ClientConfig::keep_trying](crate::ClientConfig::keep_trying).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct KeepTrying {
    /// The wait between attempts, [Backoff::NETWORK] by default.
    pub backoff: Backoff,
    /// Stop trying after this long without a link (the first connect included): the handle's streams end and
    /// [ConnectionEvent::GaveUp](crate::ConnectionEvent::GaveUp) says why. `None`, the default: never stop.
    pub give_up: Option<Duration>,
}

impl Default for KeepTrying {
    fn default() -> Self {
        KeepTrying {
            backoff: Backoff::NETWORK,
            give_up: None,
        }
    }
}

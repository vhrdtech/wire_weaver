//! Message-level transport abstraction between the sans-IO cores ([TxCore](super::core::TxCore),
//! [RxCore](super::core::RxCore)) and a medium.
//!
//! Cores speak in [ww_link] messages `(kind, bytes)`. How those get onto the wire is up to the
//! implementation: USB packs them into packets with [ww_framer], WebSocket relies on its own
//! framing and sends one message per ws frame, etc.

use std::time::Duration;

use crate::event_loop::DeviceHandle;

/// Message tx half.
pub(crate) trait MessageTx: Send + 'static {
    /// Write a message. Implementations may hold it back until a frame is full or [flush](Self::flush) is called.
    fn write_message(
        &mut self,
        kind: u8,
        message: &[u8],
    ) -> impl Future<Output = Result<(), String>> + Send;

    /// Send whatever is held back, even if the frame is not full. No-op if nothing is pending.
    fn flush(&mut self) -> impl Future<Output = Result<(), String>> + Send;

    /// Release the medium. Returns once it is actually released, so that a new connection can open it again
    /// right away (e.g., claim the same USB interface).
    fn close(self) -> impl Future<Output = ()> + Send
    where
        Self: Sized,
    {
        async {}
    }
}

/// Message rx half.
pub(crate) trait MessageRx: Send + 'static {
    /// Wait for the next message and return it, borrowed from the implementation's buffer.
    /// The message stays valid until the next call.
    ///
    /// Must be cancel-safe: dropping the future must not lose data. This is natural when
    /// any partially received state lives in `self` (e.g., a framer's staging buffer), not in the future.
    fn read_message(&mut self) -> impl Future<Output = Result<(u8, &[u8]), String>> + Send;

    /// Same as [MessageTx::close].
    fn close(self) -> impl Future<Output = ()> + Send
    where
        Self: Sized,
    {
        async {}
    }
}

pub(crate) struct Opened<Tx, Rx> {
    pub tx: Tx,
    pub rx: Rx,
}

/// Opens a connection for a handle from [Command](super::command::Command)`::Connect` and provides
/// message tx and rx halves. Called once per event loop: to re-connect, tear the loop down and start
/// a new one with the residual, possibly with a different transport.
pub(crate) trait Transport {
    type Tx: MessageTx;
    type Rx: MessageRx;

    /// How long link setup may take after [connect](Self::connect) returned, before the connection fails with
    /// [Error::LinkSetupTimeout](crate::Error::LinkSetupTimeout). Default for this transport, a user can override it
    /// with [ClientConfig::link_setup_timeout](crate::ClientConfig::link_setup_timeout). See [LinkSetupPacing].
    fn link_setup_timeout(&self) -> Duration;
    /// Must return only once the medium is usable: link setup starts right after, and requests sent before
    /// the medium is up pile up and are all answered later (e.g., while attaching to a debug probe).
    fn connect(
        &mut self,
        handle: DeviceHandle,
    ) -> impl Future<Output = Result<Opened<Self::Tx, Self::Rx>, String>> + Send;
}

/// Outcome of selecting a device for a transport.
pub(crate) enum Selected {
    /// Event loop is started, connect command must be sent to it with this handle
    Device {
        handle: DeviceHandle,
        info: Box<crate::DeviceInfo>,
    },
    /// No device matched, event loop is not started
    NotFound {
        /// WireWeaver devices (or, e.g., debug probes) that did not pass the filters
        unmatched: Vec<crate::DeviceInfo>,
    },
}

/// Default link setup timeouts, one per transport: how long it takes a device to answer the first request after the
/// medium is open. Fast media fail fast, a debug probe needs seconds (~17 ms per RTT write through a J-Link).
#[allow(dead_code)] // each one is used by a transport behind its own feature
pub(crate) mod link_setup_timeout {
    use std::time::Duration;

    pub const USB: Duration = Duration::from_secs(1);
    pub const UDP: Duration = Duration::from_secs(1);
    pub const WEB_SOCKET: Duration = Duration::from_secs(1);
    pub const IN_PROCESS: Duration = Duration::from_millis(250);
    pub const RTT: Duration = Duration::from_secs(10);
}

/// When `GetDeviceInfo` is re-sent during link setup, derived from the timeout (one setting to adjust): the first
/// retry comes after 1/20 of it (10..500 ms), then the interval doubles up to 1/4 of the timeout, so a slow medium
/// is not flooded with copies of the request, while a fast one still gets several chances.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct LinkSetupPacing {
    pub timeout: Duration,
}

impl LinkSetupPacing {
    pub fn new(timeout: Duration) -> Self {
        LinkSetupPacing { timeout }
    }

    pub fn first_interval(&self) -> Duration {
        (self.timeout / 20).clamp(Duration::from_millis(10), Duration::from_millis(500))
    }

    /// Interval after `previous`.
    pub fn next_interval(&self, previous: Duration) -> Duration {
        (previous * 2)
            .min(self.timeout / 4)
            .max(self.first_interval())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intervals(timeout: Duration) -> Vec<u128> {
        let p = LinkSetupPacing::new(timeout);
        let mut at = Duration::ZERO;
        let mut interval = p.first_interval();
        let mut out = vec![];
        while at + interval < timeout {
            at += interval;
            out.push(at.as_millis());
            interval = p.next_interval(interval);
        }
        out
    }

    #[test]
    fn defaults_per_transport() {
        use link_setup_timeout::*;
        assert!(IN_PROCESS <= USB && USB <= UDP && UDP <= WEB_SOCKET && WEB_SOCKET < RTT);
        assert_eq!(RTT, Duration::from_secs(10));
        // USB keeps its original 50 ms first retry
        assert_eq!(
            LinkSetupPacing::new(USB).first_interval(),
            Duration::from_millis(50)
        );
    }

    #[test]
    fn usb_retries_back_off_within_budget() {
        assert_eq!(intervals(link_setup_timeout::USB), [50, 150, 350, 600, 850]);
    }

    #[test]
    fn rtt_budget_is_seconds_and_retries_are_few() {
        let at = intervals(link_setup_timeout::RTT);
        assert_eq!(at, [500, 1500, 3500, 6000, 8500]);
        assert!(at.len() <= 6);
    }

    #[test]
    fn pacing_follows_override() {
        let p = LinkSetupPacing::new(Duration::from_secs(60));
        assert_eq!(p.first_interval(), Duration::from_millis(500));
        let mut i = p.first_interval();
        for _ in 0..10 {
            i = p.next_interval(i);
        }
        assert_eq!(i, Duration::from_secs(15));
        let tiny = LinkSetupPacing::new(Duration::from_millis(40));
        assert_eq!(tiny.first_interval(), Duration::from_millis(10));
    }
}

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

    /// Returns when another program asked to have the device handed to the ww hub and this holder allows it
    /// (CLT-43, the holder's [Claim](crate::holders::Claim)); the event loop then lets go of it. Never, for media
    /// that hold no claim. Must be cancel-safe.
    fn handover_requested(&mut self) -> impl Future<Output = ()> + Send {
        std::future::pending()
    }

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

/// A device's claim in the holders registry (CLT-38), shared by both halves of its link: released once both closed
/// the medium, so that whoever takes the device next (the hub after a handover, CLT-43) finds it free.
pub(crate) type SharedClaim = Option<std::sync::Arc<crate::holders::Claim>>;

/// Waits for a handover request on a shared claim, see [MessageTx::handover_requested].
#[cfg_attr(
    not(any(feature = "usb", feature = "rtt", feature = "local_socket")),
    allow(dead_code)
)]
pub(crate) async fn handover_requested(claim: &SharedClaim) {
    match claim {
        Some(claim) => claim.handover_requested().await,
        None => std::future::pending().await,
    }
}

/// A tx half of a medium that keeps the device's claim, for transports whose halves don't (RTT's stream IO).
#[cfg_attr(not(feature = "rtt"), allow(dead_code))]
pub(crate) struct HeldTx<T> {
    pub(crate) inner: T,
    pub(crate) claim: SharedClaim,
}

impl<T: MessageTx + Send> MessageTx for HeldTx<T> {
    fn write_message(
        &mut self,
        kind: u8,
        message: &[u8],
    ) -> impl Future<Output = Result<(), String>> + Send {
        self.inner.write_message(kind, message)
    }

    fn flush(&mut self) -> impl Future<Output = Result<(), String>> + Send {
        self.inner.flush()
    }

    fn handover_requested(&mut self) -> impl Future<Output = ()> + Send {
        handover_requested(&self.claim)
    }

    async fn close(self) {
        self.inner.close().await;
        drop(self.claim);
    }
}

/// The rx half of [HeldTx].
#[cfg_attr(not(feature = "rtt"), allow(dead_code))]
pub(crate) struct HeldRx<R> {
    pub(crate) inner: R,
    pub(crate) claim: SharedClaim,
}

impl<R: MessageRx + Send> MessageRx for HeldRx<R> {
    fn read_message(&mut self) -> impl Future<Output = Result<(u8, &[u8]), String>> + Send {
        self.inner.read_message()
    }

    async fn close(self) {
        self.inner.close().await;
        drop(self.claim);
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
    ///
    /// `tap` records the link (TR-14): a transport gives it the [Session](ww_tap::Session) once it knows the medium
    /// and, if it has frames, every frame both ways ([Tap::frame](ww_tap::Tap::frame), when
    /// [wanted](ww_tap::Tap::wants_frames)); messages and link events are recorded by the event loop.
    fn connect(
        &mut self,
        handle: DeviceHandle,
        tap: ww_tap::RecorderLink,
    ) -> impl Future<Output = Result<Opened<Self::Tx, Self::Rx>, String>> + Send;

    /// Whether the medium can be handed over live (CLT-44): the methods below do something.
    const LIVE: bool = false;

    /// Pauses the medium for a live handover (CLT-44), given both halves: every OUT transfer has gone out, IN is
    /// stopped and everything received so far was taken in, the messages it completed returned, nothing lost. The
    /// device may still hold IN data, which waits for the next host (NAKed). The interface may still be held.
    #[cfg_attr(not(feature = "hub"), allow(dead_code))]
    fn pause(
        _tx: &mut Self::Tx,
        _rx: &mut Self::Rx,
    ) -> impl Future<Output = Result<Paused, String>> + Send {
        async { Err("this medium can't be handed over live".to_string()) }
    }

    /// Lets go of the paused device (releases the interface), keeping what [Self::take_back] needs.
    #[cfg_attr(not(feature = "hub"), allow(dead_code))]
    fn let_go(
        _tx: &mut Self::Tx,
        _rx: &mut Self::Rx,
    ) -> impl Future<Output = Result<(), String>> + Send {
        async { Ok(()) }
    }

    /// The handover didn't happen: holds the device again (if it let go) and goes on where [Self::pause] stopped.
    #[cfg_attr(not(feature = "hub"), allow(dead_code))]
    fn take_back(
        _tx: &mut Self::Tx,
        _rx: &mut Self::Rx,
    ) -> impl Future<Output = Result<(), String>> + Send {
        async { Err("this medium can't be handed over live".to_string()) }
    }

    /// Both halves go on over the tunnel to the hub that adopted the device. The device's halves are dropped (it
    /// was let go of before).
    #[cfg(feature = "local_socket")]
    #[cfg_attr(not(feature = "hub"), allow(dead_code))]
    fn switch(
        _tx: &mut Self::Tx,
        _rx: &mut Self::Rx,
        _tunnel: crate::local_socket::Tunnel,
    ) -> Result<(), String> {
        Err("this medium can't be handed over live".to_string())
    }

    /// The holders registry claim of the device, taken out of both halves: dropping it releases the device's lock
    /// file (and tells a waiting asker of a polite handover, CLT-43).
    #[cfg_attr(not(feature = "hub"), allow(dead_code))]
    fn take_claim(_tx: &mut Self::Tx, _rx: &mut Self::Rx) -> SharedClaim {
        None
    }

    /// The holders registry claim of the device, as the halves hold it.
    #[cfg_attr(not(feature = "hub"), allow(dead_code))]
    fn claim_of(_tx: &Self::Tx) -> SharedClaim {
        None
    }

    /// Gives both halves the claim taken again after [Self::take_claim] (a live handover rolled back).
    #[cfg_attr(not(feature = "hub"), allow(dead_code))]
    fn put_claim(_tx: &mut Self::Tx, _rx: &mut Self::Rx, _claim: SharedClaim) {}
}

/// A medium paused for a live handover (CLT-44, [Transport::pause]).
#[derive(Debug)]
#[cfg_attr(not(feature = "hub"), allow(dead_code))]
pub(crate) struct Paused {
    /// The device's key in the hub (`usb:<vid>:<pid>:<serial>`), which may differ from its holders registry key
    pub key: String,
    pub medium: crate::adopt::MediumOwned,
    /// The tx framer's next frame, None without a frame prefix
    pub tx_frame: Option<ww_framer::prefix::FrameCtx>,
    pub rx: RxCarryOf,
    /// Messages the received data completed, in order
    pub messages: Vec<(u8, Vec<u8>)>,
}

#[cfg(any(feature = "usb", feature = "local_socket"))]
pub(crate) type RxCarryOf = crate::framing::RxCarry;
#[cfg(not(any(feature = "usb", feature = "local_socket")))]
pub(crate) type RxCarryOf = ();

/// How the framers of an adopted medium go on (CLT-44): wraps the transport's own handle in
/// [TxOutput::Connect](super::core::TxOutput::Connect).
#[derive(Debug, Clone, Default)]
#[cfg_attr(not(feature = "hub"), allow(dead_code))]
pub(crate) struct Carried {
    pub tx_frame: Option<ww_framer::prefix::FrameCtx>,
    pub rx: RxCarryOf,
}

/// A transport handle with [Carried] framer state.
pub(crate) struct Adopting {
    pub handle: DeviceHandle,
    #[cfg_attr(not(feature = "hub"), allow(dead_code))]
    pub carried: Carried,
}

/// A medium's halves, on the device or, after a live handover (CLT-44), on the tunnel to the hub.
#[cfg_attr(not(feature = "hub"), allow(dead_code))]
pub(crate) enum Live<D, H> {
    Device(D),
    Hub(H),
}

impl<D: MessageTx, H: MessageTx> MessageTx for Live<D, H> {
    async fn write_message(&mut self, kind: u8, message: &[u8]) -> Result<(), String> {
        match self {
            Live::Device(d) => d.write_message(kind, message).await,
            Live::Hub(h) => h.write_message(kind, message).await,
        }
    }

    async fn flush(&mut self) -> Result<(), String> {
        match self {
            Live::Device(d) => d.flush().await,
            Live::Hub(h) => h.flush().await,
        }
    }

    async fn handover_requested(&mut self) {
        match self {
            Live::Device(d) => d.handover_requested().await,
            // through the hub already
            Live::Hub(_) => std::future::pending().await,
        }
    }

    async fn close(self) {
        match self {
            Live::Device(d) => d.close().await,
            Live::Hub(h) => h.close().await,
        }
    }
}

impl<D: MessageRx, H: MessageRx> MessageRx for Live<D, H> {
    async fn read_message(&mut self) -> Result<(u8, &[u8]), String> {
        match self {
            Live::Device(d) => d.read_message().await,
            Live::Hub(h) => h.read_message().await,
        }
    }

    async fn close(self) {
        match self {
            Live::Device(d) => d.close().await,
            Live::Hub(h) => h.close().await,
        }
    }
}

/// Outcome of selecting a device for a transport.
pub(crate) enum Selected {
    /// Event loop is started, connect command must be sent to it with this handle
    Device {
        handle: DeviceHandle,
        info: Box<crate::DeviceInfo>,
        /// Through the ww hub: the client reconnects by itself when the link goes (CLT-40)
        via_hub: bool,
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
    /// Another process on the same PC, possibly the hub opening the device first
    pub const LOCAL_SOCKET: Duration = Duration::from_secs(1);
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

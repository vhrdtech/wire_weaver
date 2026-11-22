//! Message-level transport abstraction between the sans-IO cores ([TxCore](super::core::TxCore),
//! [RxCore](super::core::RxCore)) and a medium.
//!
//! Cores speak in [ww_link] messages `(kind, bytes)`. How those get onto the wire is up to the
//! implementation: USB packs them into packets with [ww_framer], WebSocket relies on its own
//! framing and sends one message per ws frame, etc.

use std::time::Duration;

use tokio::sync::oneshot;

use crate::event_loop::DeviceHandle;
use crate::event_loop::medium::{MediumRx, MediumTx};

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
    allow(dead_code, reason = "only used by some transports")
)]
pub(crate) async fn handover_requested(claim: &SharedClaim) {
    match claim {
        Some(claim) => claim.handover_requested().await,
        None => std::future::pending().await,
    }
}

/// A tx half of a medium that keeps the device's claim, for transports whose halves don't (RTT's stream IO).
#[cfg_attr(
    not(feature = "rtt"),
    allow(dead_code, reason = "only used with the `rtt` feature")
)]
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
#[cfg_attr(
    not(feature = "rtt"),
    allow(dead_code, reason = "only used with the `rtt` feature")
)]
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
///
/// The halves go into the event loop's one transport slot ([MediumTx], [MediumRx]), which a live handover swaps for
/// another medium's ([relink](super::medium::relink)); the steps below are this medium's part in that, called on
/// whichever medium is in the slot.
pub(crate) trait Transport {
    /// Goes into the event loop's slot ([MediumTx]): a medium of this crate has a variant of its own
    type Tx: MessageTx + Into<MediumTx>;
    type Rx: MessageRx + Into<MediumRx>;

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
    ///
    /// `max_message_len` is the longest message this host takes
    /// ([ClientConfig::max_message_len](crate::ClientConfig::max_message_len), sent in link setup): receive buffers
    /// hold one such message and what the medium adds to it.
    fn connect(
        &mut self,
        handle: DeviceHandle,
        tap: crate::tap::ClientTap,
        max_message_len: usize,
    ) -> impl Future<Output = Result<Opened<Self::Tx, Self::Rx>, String>> + Send;

    /// Whether the medium can be handed over live (CLT-44): the methods below do something.
    const LIVE: bool = false;

    /// Pauses the medium for a live handover (CLT-44), given both halves: every OUT transfer has gone out, IN is
    /// stopped and everything received so far was taken in, the messages it completed returned, nothing lost. The
    /// device may still hold IN data, which waits for the next host (NAKed). The interface may still be held.
    #[cfg_attr(
        not(feature = "hub"),
        allow(dead_code, reason = "only used with the `hub` feature")
    )]
    fn pause(
        _tx: &mut Self::Tx,
        _rx: &mut Self::Rx,
    ) -> impl Future<Output = Result<Paused, String>> + Send {
        async { Err("this medium can't be handed over live".to_string()) }
    }

    /// Lets go of the paused device (releases the interface), keeping what [Self::take_back] needs.
    #[cfg_attr(
        not(feature = "hub"),
        allow(dead_code, reason = "only used with the `hub` feature")
    )]
    fn let_go(
        _tx: &mut Self::Tx,
        _rx: &mut Self::Rx,
    ) -> impl Future<Output = Result<(), String>> + Send {
        async { Ok(()) }
    }

    /// The handover didn't happen: holds the device again (if it let go) and goes on where [Self::pause] stopped.
    #[cfg_attr(
        not(feature = "hub"),
        allow(dead_code, reason = "only used with the `hub` feature")
    )]
    fn take_back(
        _tx: &mut Self::Tx,
        _rx: &mut Self::Rx,
    ) -> impl Future<Output = Result<(), String>> + Send {
        async { Err("this medium can't be handed over live".to_string()) }
    }

    /// The link leaves this medium for another one ([relink](super::medium::relink)), after [Self::let_go]: what the
    /// halves still have to do before they are dropped. An error keeps them in the slot.
    #[cfg_attr(
        not(feature = "hub"),
        allow(dead_code, reason = "only used with the `hub` feature")
    )]
    fn switch(_tx: &mut Self::Tx, _rx: &mut Self::Rx) -> Result<(), String> {
        Err("this medium can't be handed over live".to_string())
    }

    /// The holders registry claim of the device, taken out of both halves: dropping it releases the device's lock
    /// file (and tells a waiting asker of a polite handover, CLT-43).
    #[cfg_attr(
        not(feature = "hub"),
        allow(dead_code, reason = "only used with the `hub` feature")
    )]
    fn take_claim(_tx: &mut Self::Tx, _rx: &mut Self::Rx) -> SharedClaim {
        None
    }

    /// The holders registry claim of the device, as the halves hold it.
    #[cfg_attr(
        not(feature = "hub"),
        allow(dead_code, reason = "only used with the `hub` feature")
    )]
    fn claim_of(_tx: &Self::Tx) -> SharedClaim {
        None
    }

    /// Gives both halves the claim taken again after [Self::take_claim] (a live handover rolled back).
    #[cfg_attr(
        not(feature = "hub"),
        allow(dead_code, reason = "only used with the `hub` feature")
    )]
    fn put_claim(_tx: &mut Self::Tx, _rx: &mut Self::Rx, _claim: SharedClaim) {}
}

/// A medium paused for a live handover (CLT-44, [Transport::pause]).
#[derive(Debug)]
#[cfg_attr(
    not(feature = "hub"),
    allow(dead_code, reason = "only used with the `hub` feature")
)]
pub(crate) struct Paused {
    /// The device's key in the hub (`usb:<vid>:<pid>:<serial>`), which may differ from its holders registry key
    pub key: String,
    /// What the medium that takes the link over goes on from (its endpoints, the framers' state)
    pub medium: crate::adopt::MediumResidualOwned,
    /// Messages the received data completed, in order
    pub messages: Vec<(u8, Vec<u8>)>,
}

/// A transport handle for a link adopted from another program (CLT-44): the medium goes on from `medium`, which it
/// decodes itself, see [TxOutput::Connect](super::core::TxOutput::Connect).
#[cfg_attr(
    not(any(feature = "usb", feature = "local_socket")),
    allow(
        dead_code,
        reason = "only used with the `usb` or `local_socket` features"
    )
)]
pub(crate) struct Adopting {
    pub handle: DeviceHandle,
    pub medium: crate::adopt::MediumResidualOwned,
}

/// One address of a [Race]: what it is called and the transport's handle for it.
#[cfg_attr(
    not(any(feature = "ws", feature = "in_process")),
    allow(dead_code, reason = "only made by the ws and in_process transports")
)]
pub(crate) struct Candidate {
    /// As the config names it (a URL, an in-process path): what [ValidatedConfig::without_address] takes
    ///
    /// [ValidatedConfig::without_address]: crate::config::ValidatedConfig::without_address
    pub(crate) address: String,
    /// As [DeviceInfo::location](crate::DeviceInfo::location) names it
    pub(crate) location: String,
    pub(crate) handle: DeviceHandle,
}

/// What a [Race] opened, see [Candidates::watch].
#[derive(Clone, Debug)]
pub(crate) struct Won {
    pub(crate) address: String,
    pub(crate) location: String,
}

/// The handle of a [Race]: the addresses of one device, opened at once (CLT-80).
pub(crate) struct Candidates {
    entries: Vec<Candidate>,
    /// Told the index of the entry that opened
    opened: Option<oneshot::Sender<usize>>,
}

impl Candidates {
    #[cfg_attr(
        not(any(feature = "ws", feature = "in_process")),
        allow(dead_code, reason = "only made by the ws and in_process transports")
    )]
    pub(crate) fn new(entries: Vec<Candidate>) -> Self {
        Candidates {
            entries,
            opened: None,
        }
    }

    /// Where each entry is, and a receiver of the one that opens: taken before the handle goes to the event loop.
    pub(crate) fn watch(&mut self) -> (Vec<Won>, oneshot::Receiver<usize>) {
        let (tx, rx) = oneshot::channel();
        self.opened = Some(tx);
        let entries = self
            .entries
            .iter()
            .map(|c| Won {
                address: c.address.clone(),
                location: c.location.clone(),
            })
            .collect();
        (entries, rx)
    }
}

/// A transport that opens every address of [Candidates] with `T` at once (CLT-80): the first to open is the medium,
/// the others still opening are dropped. A tie goes to the earlier address. Each opening may take `open_timeout`;
/// when all fail, the error names each.
#[cfg(any(feature = "ws", feature = "in_process"))]
pub(crate) struct Race<T> {
    pub(crate) transport: T,
    pub(crate) open_timeout: Duration,
}

#[cfg(any(feature = "ws", feature = "in_process"))]
impl<T: Transport + Clone + Send + 'static> Transport for Race<T> {
    type Tx = T::Tx;
    type Rx = T::Rx;

    fn link_setup_timeout(&self) -> Duration {
        self.transport.link_setup_timeout()
    }

    async fn connect(
        &mut self,
        handle: DeviceHandle,
        tap: crate::tap::ClientTap,
        max_message_len: usize,
    ) -> Result<Opened<T::Tx, T::Rx>, String> {
        type Opening<Tx, Rx> =
            std::pin::Pin<Box<dyn Future<Output = Result<Opened<Tx, Rx>, String>> + Send>>;
        let Candidates { entries, opened } = *handle
            .downcast::<Candidates>()
            .map_err(|_| "expected the addresses to race".to_string())?;
        let timeout = self.open_timeout;
        let mut openings: Vec<Option<Opening<T::Tx, T::Rx>>> = entries
            .into_iter()
            .map(|c| {
                let mut transport = self.transport.clone();
                let tap = tap.clone();
                let opening: Opening<T::Tx, T::Rx> = Box::pin(async move {
                    tokio::time::timeout(timeout, transport.connect(c.handle, tap, max_message_len))
                        .await
                        .unwrap_or_else(|_| {
                            Err(format!("{}: no answer within {timeout:?}", c.address))
                        })
                });
                Some(opening)
            })
            .collect();
        let mut errors = Vec::new();
        let won = std::future::poll_fn(|cx| {
            // in order, so a tie goes to the earlier address
            for (i, slot) in openings.iter_mut().enumerate() {
                let Some(opening) = slot else { continue };
                if let std::task::Poll::Ready(r) = opening.as_mut().poll(cx) {
                    *slot = None;
                    match r {
                        Ok(o) => return std::task::Poll::Ready(Some((i, o))),
                        Err(e) => errors.push(e),
                    }
                }
            }
            if openings.iter().all(Option::is_none) {
                std::task::Poll::Ready(None)
            } else {
                std::task::Poll::Pending
            }
        })
        .await;
        // the ones still opening go with this
        drop(openings);
        match won {
            Some((i, o)) => {
                if let Some(opened) = opened {
                    _ = opened.send(i);
                }
                Ok(o)
            }
            None if errors.is_empty() => Err("no address to open".into()),
            None => Err(errors.join("; ")),
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
#[allow(
    dead_code,
    reason = "each one is used by a transport behind its own feature"
)]
pub(crate) mod link_setup_timeout {
    use std::time::Duration;

    pub const USB: Duration = Duration::from_secs(1);
    pub const UDP: Duration = Duration::from_secs(1);
    pub const WEB_SOCKET: Duration = Duration::from_secs(1);
    pub const IN_PROCESS: Duration = Duration::from_millis(250);
    /// Another process on the same PC, possibly the hub opening the device first
    pub const LOCAL_SOCKET: Duration = Duration::from_secs(1);
    /// TCP, possibly across networks, or a command that starts first (ssh)
    pub const BYTE_STREAM: Duration = Duration::from_secs(5);
    pub const RTT: Duration = Duration::from_secs(10);
}

/// When the hello is re-sent during link setup, derived from the timeout (one setting to adjust): the first
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

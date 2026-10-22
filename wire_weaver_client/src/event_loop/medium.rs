//! The event loop's transport slot: one tx and one rx half of any medium ([MediumTx], [MediumRx]), swapped for
//! another medium's while the link goes on ([relink], CLT-44).
//!
//! The slot is an enum over this crate's media, not a trait object: a message write or read is one match on the
//! variant, with no allocation, the same as the `Live<Device, Hub>` enum each live medium had before. A boxed future
//! per call would add an allocation to every message both ways. Media from outside this list (tests) go in
//! [MediumTx::Other] / [MediumRx::Other], boxed.
//!
//! The per-medium steps of a live handover ([Transport::pause], [Transport::let_go], [Transport::take_back],
//! [Transport::switch], the claim) are the [Transport]'s of the medium in the slot, dispatched by variant.

use std::future::Future;
use std::pin::Pin;

use super::core::TxCore;
#[cfg_attr(
    not(any(
        feature = "usb",
        feature = "local_socket",
        feature = "rtt",
        feature = "ws",
        feature = "udp",
        feature = "in_process"
    )),
    allow(unused_imports)
)]
use super::transport::Transport;
use super::transport::{MessageRx, MessageTx, Paused, SharedClaim};

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A [MessageTx] behind a box, for [MediumTx::Other].
pub(crate) trait DynTx: Send + 'static {
    fn write_boxed<'a>(
        &'a mut self,
        kind: u8,
        message: &'a [u8],
    ) -> BoxFuture<'a, Result<(), String>>;
    fn flush_boxed(&mut self) -> BoxFuture<'_, Result<(), String>>;
    fn handover_requested_boxed(&mut self) -> BoxFuture<'_, ()>;
    fn close_boxed(self: Box<Self>) -> BoxFuture<'static, ()>;
}

impl<T: MessageTx> DynTx for T {
    fn write_boxed<'a>(
        &'a mut self,
        kind: u8,
        message: &'a [u8],
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(self.write_message(kind, message))
    }

    fn flush_boxed(&mut self) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(self.flush())
    }

    fn handover_requested_boxed(&mut self) -> BoxFuture<'_, ()> {
        Box::pin(self.handover_requested())
    }

    fn close_boxed(self: Box<Self>) -> BoxFuture<'static, ()> {
        Box::pin((*self).close())
    }
}

/// A [MessageRx] behind a box, for [MediumRx::Other].
pub(crate) trait DynRx: Send + 'static {
    fn read_boxed(&mut self) -> BoxFuture<'_, Result<(u8, &[u8]), String>>;
    fn close_boxed(self: Box<Self>) -> BoxFuture<'static, ()>;
}

impl<R: MessageRx> DynRx for R {
    fn read_boxed(&mut self) -> BoxFuture<'_, Result<(u8, &[u8]), String>> {
        Box::pin(self.read_message())
    }

    fn close_boxed(self: Box<Self>) -> BoxFuture<'static, ()> {
        Box::pin((*self).close())
    }
}

/// Puts halves of a medium that isn't one of [MediumTx]'s variants in the slot boxed (tests).
#[cfg(test)]
macro_rules! other_medium {
    ($(tx: $tx:ty;)? $(rx: $rx:ty;)?) => {
        $(impl From<$tx> for $crate::event_loop::medium::MediumTx {
            fn from(tx: $tx) -> Self {
                $crate::event_loop::medium::MediumTx::Other(Box::new(tx))
            }
        })?
        $(impl From<$rx> for $crate::event_loop::medium::MediumRx {
            fn from(rx: $rx) -> Self {
                $crate::event_loop::medium::MediumRx::Other(Box::new(rx))
            }
        })?
    };
}
#[cfg(test)]
pub(crate) use other_medium;

/// The slot's enums, the halves' conversions into them and the dispatch, for each medium: its variant, halves and
/// [Transport].
macro_rules! media {
    ($($(#[$cfg:meta])* $variant:ident($tx:ty, $rx:ty, $transport:ty),)*) => {
        /// The tx half in the event loop's slot.
        pub(crate) enum MediumTx {
            $($(#[$cfg])* $variant($tx),)*
            /// A medium not in this list
            #[cfg_attr(not(test), allow(dead_code))]
            Other(Box<dyn DynTx>),
        }

        /// The rx half in the event loop's slot.
        pub(crate) enum MediumRx {
            $($(#[$cfg])* $variant($rx),)*
            /// A medium not in this list
            #[cfg_attr(not(test), allow(dead_code))]
            Other(Box<dyn DynRx>),
        }

        $(
            $(#[$cfg])*
            impl From<$tx> for MediumTx {
                fn from(tx: $tx) -> Self {
                    MediumTx::$variant(tx)
                }
            }

            $(#[$cfg])*
            impl From<$rx> for MediumRx {
                fn from(rx: $rx) -> Self {
                    MediumRx::$variant(rx)
                }
            }
        )*

        impl MessageTx for MediumTx {
            async fn write_message(&mut self, kind: u8, message: &[u8]) -> Result<(), String> {
                match self {
                    $($(#[$cfg])* MediumTx::$variant(tx) => tx.write_message(kind, message).await,)*
                    MediumTx::Other(tx) => tx.write_boxed(kind, message).await,
                }
            }

            async fn flush(&mut self) -> Result<(), String> {
                match self {
                    $($(#[$cfg])* MediumTx::$variant(tx) => tx.flush().await,)*
                    MediumTx::Other(tx) => tx.flush_boxed().await,
                }
            }

            async fn handover_requested(&mut self) {
                match self {
                    $($(#[$cfg])* MediumTx::$variant(tx) => tx.handover_requested().await,)*
                    MediumTx::Other(tx) => tx.handover_requested_boxed().await,
                }
            }

            async fn close(self) {
                match self {
                    $($(#[$cfg])* MediumTx::$variant(tx) => tx.close().await,)*
                    MediumTx::Other(tx) => tx.close_boxed().await,
                }
            }
        }

        impl MessageRx for MediumRx {
            async fn read_message(&mut self) -> Result<(u8, &[u8]), String> {
                match self {
                    $($(#[$cfg])* MediumRx::$variant(rx) => rx.read_message().await,)*
                    MediumRx::Other(rx) => rx.read_boxed().await,
                }
            }

            async fn close(self) {
                match self {
                    $($(#[$cfg])* MediumRx::$variant(rx) => rx.close().await,)*
                    MediumRx::Other(rx) => rx.close_boxed().await,
                }
            }
        }

        impl MediumTx {
            /// Whether the medium in the slot can be handed over live ([Transport::LIVE]).
            pub(crate) fn live(&self) -> bool {
                match self {
                    $($(#[$cfg])* MediumTx::$variant(_) => <$transport as Transport>::LIVE,)*
                    MediumTx::Other(_) => false,
                }
            }

            /// [Transport::claim_of]
            #[cfg_attr(not(feature = "hub"), allow(dead_code))]
            pub(crate) fn claim(&self) -> SharedClaim {
                match self {
                    $($(#[$cfg])* MediumTx::$variant(tx) => <$transport as Transport>::claim_of(tx),)*
                    MediumTx::Other(_) => None,
                }
            }
        }

        /// The halves in the slot, of one medium, as that medium's [Transport] takes them; `$other` for halves of two
        /// media or of an [MediumTx::Other] one.
        #[cfg_attr(not(feature = "hub"), allow(unused_macros))]
        macro_rules! with_medium {
            ($tx_:expr, $rx_:expr, |$t:ident, $r:ident, $T:ident| $body:expr, $other:expr) => {
                match ($tx_, $rx_) {
                    $($(#[$cfg])* (MediumTx::$variant($t), MediumRx::$variant($r)) => {
                        type $T = $transport;
                        $body
                    })*
                    _ => $other,
                }
            };
        }
    };
}

media! {
    #[cfg(feature = "usb")]
    Usb(crate::usb::NusbTx, crate::usb::NusbRx, crate::usb::NusbTransport),
    #[cfg(feature = "local_socket")]
    UsbSim(crate::usb_sim::UsbSimTx, crate::usb_sim::UsbSimRx, crate::usb_sim::UsbSimTransport),
    #[cfg(feature = "local_socket")]
    Local(crate::local_socket::LocalTx, crate::local_socket::LocalRx, crate::local_socket::LocalSocketTransport),
    #[cfg(feature = "rtt")]
    Rtt(crate::rtt::Tx, crate::rtt::Rx, crate::rtt::RttTransport),
    #[cfg(feature = "ws")]
    Ws(crate::ws::WsTx, crate::ws::WsRx, crate::ws::WsTransport),
    #[cfg(feature = "udp")]
    Udp(crate::udp::UdpTx, crate::udp::UdpRx, crate::udp::UdpTransport),
    #[cfg(feature = "in_process")]
    InProcess(crate::in_process::HostTx, crate::in_process::HostRx, crate::in_process::InProcessTransport),
}

const NOT_LIVE: &str = "this medium can't be handed over live";

/// [Transport::pause] of the medium in the slot.
#[cfg_attr(not(feature = "hub"), allow(dead_code))]
pub(crate) async fn pause(tx: &mut MediumTx, rx: &mut MediumRx) -> Result<Paused, String> {
    with_medium!(tx, rx, |t, r, T| T::pause(t, r).await, Err(NOT_LIVE.into()))
}

/// [Transport::let_go] of the medium in the slot.
#[cfg_attr(not(feature = "hub"), allow(dead_code))]
pub(crate) async fn let_go(tx: &mut MediumTx, rx: &mut MediumRx) -> Result<(), String> {
    with_medium!(
        tx,
        rx,
        |t, r, T| T::let_go(t, r).await,
        Err(NOT_LIVE.into())
    )
}

/// [Transport::take_back] of the medium in the slot.
#[cfg_attr(not(feature = "hub"), allow(dead_code))]
pub(crate) async fn take_back(tx: &mut MediumTx, rx: &mut MediumRx) -> Result<(), String> {
    with_medium!(
        tx,
        rx,
        |t, r, T| T::take_back(t, r).await,
        Err(NOT_LIVE.into())
    )
}

/// [Transport::take_claim] of the medium in the slot.
#[cfg_attr(not(feature = "hub"), allow(dead_code))]
pub(crate) fn take_claim(tx: &mut MediumTx, rx: &mut MediumRx) -> SharedClaim {
    with_medium!(tx, rx, |t, r, T| T::take_claim(t, r), None)
}

/// [Transport::put_claim] of the medium in the slot.
#[cfg_attr(not(feature = "hub"), allow(dead_code))]
pub(crate) fn put_claim(tx: &mut MediumTx, rx: &mut MediumRx, claim: SharedClaim) {
    with_medium!(tx, rx, |t, r, T| T::put_claim(t, r, claim), drop(claim))
}

/// The link goes on over another medium, from whichever medium is in the slot to whichever `to` is (CLT-44): the
/// halves in the slot leave theirs ([Transport::switch], after [Transport::let_go]) and are dropped, `to`'s take their
/// place, and the tx core links up over them with the same seqs, requests in flight and what waits for them
/// ([TxCore::relink]).
#[cfg_attr(not(feature = "hub"), allow(dead_code))]
pub(crate) fn relink(
    core: &mut TxCore,
    tx: &mut MediumTx,
    rx: &mut MediumRx,
    to: (MediumTx, MediumRx),
) -> Result<(), String> {
    with_medium!(
        &mut *tx,
        &mut *rx,
        |t, r, T| T::switch(t, r),
        Err(NOT_LIVE.to_string())
    )?;
    (*tx, *rx) = to;
    core.relink(super::core::now());
    Ok(())
}

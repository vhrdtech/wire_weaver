//! Live link handover, the holder's side (CLT-44): when another program asks this direct holder to hand its device to
//! the ww hub (CLT-43's request), the event loop hands the hub its live link instead of releasing the device, and goes
//! on through a tunnel to the hub with the same seqs, requests in flight, streams and observers: the app sees a
//! pause, the device sees nothing.
//!
//! Two-phase, so that either side dying leaves one working holder:
//! 1. connect to the hub API on a task of its own ([Connecting]), the link running on meanwhile;
//! 2. pause: the tx core holds new requests and pings; the medium sends every OUT transfer and reaps IN
//!    ([Transport::pause]); what came in is dispatched; the state of both cores is the link residual, what the medium
//!    leaves (its framers' state, endpoints) the medium residual ([HandoverOwned](crate::adopt::HandoverOwned));
//! 3. `Adopt::prepare(handover)`: the hub checks it and keeps the device for us;
//! 4. let go of the device ([Transport::let_go]) and of its lock file (a waiting asker hears `Released`), then
//!    `Adopt::commit`: the hub opens the device going on with the link;
//! 5. open the tunnel (preamble with the adoption id) and relink over it ([relink]): the slot's halves leave the
//!    device ([Transport::switch]), the tunnel's take their place, and the cores link up with the hub with the same
//!    seqs ([TxCore::relink]).
//!
//! Every step is the medium's in the event loop's slot ([MediumTx](super::medium::MediumTx)), whichever it is: this
//! module knows nothing of USB.
//!
//! [Transport::pause]: super::transport::Transport::pause
//! [Transport::let_go]: super::transport::Transport::let_go
//! [Transport::switch]: super::transport::Transport::switch
//! [TxCore::relink]: super::core::TxCore::relink
//! [relink]: super::medium::relink
//!
//! The hub refusing before the commit: back to the medium, then CLT-43's release and reopen. The hub gone before the
//! commit: back to the medium, holding the device again (the asker hears `Failed`). After the commit the device is
//! the hub's: a failed tunnel ends this link as `HandedToHub` without a `Disconnect` to the device, and the client
//! reconnects through the hub (CLT-40).

use tokio::sync::oneshot;

use super::core::RxPart;
use super::medium::MediumRx;
#[cfg(feature = "hub")]
pub(crate) use with_hub::hand_over;

/// What the tx task asks of the rx task while it hands the link over.
#[cfg_attr(not(feature = "hub"), allow(dead_code))]
pub(crate) enum RxCtl {
    /// Give the rx half to the tx task (dropped if there is none)
    Lend(oneshot::Sender<MediumRx>),
    /// Take it back, maybe another medium's now
    Return(MediumRx),
    /// Messages the medium completed while it was paused, in order
    Feed(Vec<(u8, Vec<u8>)>),
    /// The rx core's part of the link residual, after everything fed before
    Part(oneshot::Sender<RxPart>),
}

/// The hub API connection a handover waits for, made on a task of its own, so that the link runs on meanwhile and
/// only the pause itself holds it up.
#[derive(Default)]
pub(crate) struct Connecting(
    #[cfg(feature = "hub")]
    Option<
        tokio::task::JoinHandle<Result<crate::hub::adopt::HubSide, crate::hub::adopt::Refused>>,
    >,
);

impl Connecting {
    pub(crate) fn is_pending(&self) -> bool {
        #[cfg(feature = "hub")]
        return self.0.is_some();
        #[cfg(not(feature = "hub"))]
        false
    }

    /// Starts connecting to the hub. False without the `hub` feature or with [LIVE_HANDOVER_ENV] `off`: nothing to
    /// hand over to live.
    ///
    /// [LIVE_HANDOVER_ENV]: crate::hub::LIVE_HANDOVER_ENV
    pub(crate) fn start(&mut self) -> bool {
        #[cfg(feature = "hub")]
        {
            if std::env::var(crate::hub::LIVE_HANDOVER_ENV).is_ok_and(|v| v == "off") {
                return false;
            }
            self.0 = Some(tokio::spawn(crate::hub::adopt::connect()));
            true
        }
        #[cfg(not(feature = "hub"))]
        false
    }

    /// Once connected (or not): never while nothing was started. Cancel-safe.
    #[cfg(feature = "hub")]
    pub(crate) async fn wait(
        &mut self,
    ) -> Result<crate::hub::adopt::HubSide, crate::hub::adopt::Refused> {
        let Some(task) = self.0.as_mut() else {
            return std::future::pending().await;
        };
        let r = task.await;
        self.0 = None;
        r.unwrap_or_else(|e| Err(crate::hub::adopt::Refused::ByHub(e.to_string())))
    }

    #[cfg(not(feature = "hub"))]
    pub(crate) async fn wait(&mut self) -> std::convert::Infallible {
        std::future::pending().await
    }
}

/// How a handover went.
#[cfg_attr(not(feature = "hub"), allow(dead_code))]
pub(crate) enum Outcome {
    /// The link goes on through the hub
    Switched,
    /// The link goes on over the device as before
    Kept,
    /// Hand it over by releasing it (CLT-43)
    Release,
    /// The hub has the device, this program couldn't go on through it: end the link without a `Disconnect`
    HandedOff,
}

#[cfg(feature = "hub")]
mod with_hub {
    use std::time::Instant;

    use tokio::sync::{mpsc, oneshot};
    use tracing::{debug, info, warn};
    use ww_tap::{Dir, RecorderLink, Tap};

    use super::{Outcome, RxCtl};
    use crate::adopt::{HandoverOwned, LinkResidualOwned, TraitIdOwned};
    use crate::event_loop::core::{RxPart, ToRx, TxCore, TxOutput, TxPart, now};
    use crate::event_loop::medium::{self, MediumRx, MediumTx};
    use crate::event_loop::transport::MessageTx;
    use crate::hub::adopt::{HubSide, Refused};

    /// Hands the link over, see the [module docs](super).
    pub(crate) async fn hand_over(
        core: &mut TxCore,
        tx: &mut MediumTx,
        to_rx: &mpsc::UnboundedSender<ToRx>,
        rx_ctl: &mpsc::UnboundedSender<RxCtl>,
        tap: &mut Option<RecorderLink>,
        hub: Result<HubSide, Refused>,
    ) -> Outcome {
        let Some(claim) = tx.claim() else {
            return Outcome::Release;
        };
        let key = claim.key().to_string();
        let hub = match hub {
            Ok(hub) => hub,
            Err(e) => {
                info!("{key}: not handing the live link to the ww hub ({e}), releasing the device");
                return Outcome::Release;
            }
        };
        let started = Instant::now();
        if !core.pause(now()) {
            debug!("{key}: the link can't be paused now (not up, or a loopback test runs)");
            return Outcome::Release;
        }
        if let Err(e) = write_outputs(core, tx, to_rx, tap).await {
            warn!("{key}: pausing the link: {e}");
            return Outcome::Release;
        }
        let (lend_tx, lend_rx) = oneshot::channel();
        if rx_ctl.send(RxCtl::Lend(lend_tx)).is_err() {
            core.resume(now());
            return Outcome::Release;
        }
        let Ok(mut rx) = lend_rx.await else {
            core.resume(now());
            return Outcome::Release;
        };
        let give_back = |rx: MediumRx| _ = rx_ctl.send(RxCtl::Return(rx));
        let paused = match medium::pause(tx, &mut rx).await {
            Ok(paused) => paused,
            Err(e) => {
                // the medium broke on the way: whatever the next read says ends the link
                warn!("{key}: pausing the medium: {e}");
                give_back(rx);
                core.resume(now());
                return Outcome::Release;
            }
        };
        let (part_tx, part_rx) = oneshot::channel();
        _ = rx_ctl.send(RxCtl::Feed(paused.messages));
        _ = rx_ctl.send(RxCtl::Part(part_tx));
        let Ok(rx_part) = part_rx.await else {
            give_back(rx);
            core.resume(now());
            return Outcome::Release;
        };
        let handover = HandoverOwned {
            client_version: crate::adopt::CLIENT_VERSION.into(),
            key: paused.key,
            link: link_residual(core.link_part(), rx_part),
            medium: paused.medium,
        };
        let in_flight = handover.link.in_flight.len();
        let hub_key = handover.key.clone();
        let id = match hub.prepare(&handover).await {
            Ok(id) => id,
            Err(e) => {
                let outcome = match e {
                    Refused::ByHub(_) => {
                        info!("{key}: {e}, releasing the device instead");
                        Outcome::Release
                    }
                    Refused::Gone(_) => {
                        warn!("{key}: {e} before taking the live link: keeping the device");
                        claim.handover_failed();
                        Outcome::Kept
                    }
                };
                return match medium::take_back(tx, &mut rx).await {
                    Ok(()) => {
                        give_back(rx);
                        core.resume(now());
                        outcome
                    }
                    Err(e) => {
                        warn!("{key}: taking the device back: {e}");
                        give_back(rx);
                        core.resume(now());
                        Outcome::Release
                    }
                };
            }
        };
        if let Err(e) = medium::let_go(tx, &mut rx).await {
            warn!("{key}: letting go of the device: {e}");
        }
        // the lock file goes: the hub takes it, a waiting asker hears Released
        let again = claim.again();
        drop(claim);
        drop(medium::take_claim(tx, &mut rx));
        if let Err(e) = hub.commit(id).await {
            warn!("{key}: {e} taking the live link: keeping the device");
            match again.claim() {
                Ok(c) => medium::put_claim(tx, &mut rx, Some(std::sync::Arc::new(c))),
                Err(e) => warn!("{key}: holding it in the holders registry again: {e}"),
            }
            return match medium::take_back(tx, &mut rx).await {
                Ok(()) => {
                    give_back(rx);
                    core.resume(now());
                    Outcome::Kept
                }
                Err(e) => {
                    warn!("{key}: taking the device back: {e}");
                    give_back(rx);
                    Outcome::HandedOff
                }
            };
        }
        let tunnel = match hub.tunnel(&hub_key, id).await {
            Ok(tunnel) => tunnel,
            Err(e) => {
                warn!("{key}: the ww hub took the live link, but its tunnel failed: {e}");
                give_back(rx);
                return Outcome::HandedOff;
            }
        };
        let (htx, hrx) = tunnel.into_halves(None);
        if let Err(e) = medium::relink(core, tx, &mut rx, (htx.into(), hrx.into())) {
            warn!("{key}: {e}");
            give_back(rx);
            return Outcome::HandedOff;
        }
        give_back(rx);
        // the hub API connection is done; a Commander's drop sleeps a moment for its Disconnect, not on this task
        tokio::task::spawn_blocking(move || drop(hub));
        info!(
            "{key}: handed the live link to the ww hub in {:?} ({in_flight} requests in flight), going on through it",
            started.elapsed()
        );
        Outcome::Switched
    }

    /// The link's residual, from the parts of the paused cores: nothing of the medium.
    fn link_residual(tx: TxPart, rx: RxPart) -> LinkResidualOwned {
        let ms = |d: std::time::Duration| d.as_millis().min(u32::MAX as u128) as u32;
        LinkResidualOwned {
            token: tx.token,
            device_info: tx.device_info,
            ping_in_ms: ms(tx.ping_in),
            last_rx_ms_ago: ms(rx.last_rx_ago),
            next_seq: tx.next_seq,
            max_seq: tx.max_seq,
            in_flight: tx.in_flight,
            full_only: tx
                .full_only
                .into_iter()
                .map(|(id, major, minor)| TraitIdOwned { id, major, minor })
                .collect(),
            streams: rx.streams,
            observed: rx.observed,
        }
    }

    /// Writes what the paused core still had for the medium.
    async fn write_outputs<Tx: MessageTx>(
        core: &mut TxCore,
        tx: &mut Tx,
        to_rx: &mpsc::UnboundedSender<ToRx>,
        tap: &mut Option<RecorderLink>,
    ) -> Result<(), String> {
        while let Some(output) = core.poll_output() {
            match output {
                TxOutput::Send { kind, message } => {
                    if tap.wants_messages() {
                        tap.message(Dir::Tx, kind, &message);
                    }
                    tx.write_message(kind, &message).await?
                }
                TxOutput::Flush => tx.flush().await?,
                TxOutput::ToRx(msg) => {
                    if let Err(undelivered) = to_rx.send(msg) {
                        undelivered.0.fail_undelivered();
                    }
                }
                TxOutput::Connect(..) | TxOutput::Exit(_) => {
                    return Err("unexpected output of a paused link".into());
                }
            }
        }
        Ok(())
    }
}

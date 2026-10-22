use std::{any::Any, marker::PhantomData};

use crate::config::InterfaceKind;
use crate::event_loop::command::EventLoopExitReason;
use crate::event_loop::commander::WeakCommander;
use crate::event_loop::transport::Selected;
use crate::{
    Commander, ConnectionEvent, GapReason, WwClient,
    config::{ClientConfig, IntrospectBundle, ValidatedConfig},
    device_info::DeviceApiInfo,
    event_loop::command::{Command, EventLoopResidual},
};
use anyhow::{Error, Result};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tracing::{debug, info, warn};

pub struct PreparedConnection<T> {
    config: ClientConfig,
    _phantom: PhantomData<T>,
}

impl<T: WwClient> PreparedConnection<T> {
    pub fn new(config: ClientConfig) -> Self {
        Self {
            config,
            _phantom: PhantomData,
        }
    }

    pub async fn connect(self) -> Result<T> {
        let config = self.config.validate()?;
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(config.cmd_queue_size);
        let mut cmd_rx = Some(cmd_rx);
        let attempt = connect_to(&config, &cmd_tx, &mut cmd_rx).await;
        let (device_api_info, residual_rx) = attempt.result?;
        let reconnect = Reconnect::new(config.clone(), residual_rx, attempt.via_hub);
        let supervise = config.hub_reconnect.is_some();
        #[cfg(feature = "clock_sync")]
        let clock = ClockStart::new(&config, attempt.key, reconnect.via_hub.clone());
        let mut commander = create_commander(config, cmd_tx, device_api_info).await;
        if supervise {
            reconnect.supervise(commander.downgrade());
        }
        commander.set_reconnect(reconnect);
        #[cfg(feature = "clock_sync")]
        clock.start(&mut commander);
        Ok(T::from_cmd(commander))
    }

    pub fn connect_blocking(self) -> Result<T> {
        let config = self.config.validate()?;
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(config.cmd_queue_size);
        let mut cmd_rx = Some(cmd_rx);
        let mut unmatched = vec![];

        for iface_kind in check_interfaces(&config)? {
            let selected = match iface_kind {
                #[cfg(feature = "usb")]
                InterfaceKind::Usb => crate::usb::try_connect_blocking(&config, &mut cmd_rx)?,
                #[cfg(not(feature = "usb"))]
                InterfaceKind::Usb => continue,
                InterfaceKind::InProcess => try_select_in_process(&config, &mut cmd_rx)?,
                InterfaceKind::Rtt => try_select_rtt(&config, &mut cmd_rx)?,
                InterfaceKind::WebSocket => try_select_ws(&config, &mut cmd_rx)?,
                InterfaceKind::Udp => try_select_udp(&config, &mut cmd_rx)?,
                InterfaceKind::Ipc => {
                    // the tunnel is opened before the event loop runs (CLT-40), on the runtime the event loop runs
                    // on, which a multi-thread one drives while this thread waits
                    let rt = tokio::runtime::Handle::current();
                    if rt.runtime_flavor() != tokio::runtime::RuntimeFlavor::MultiThread {
                        return Err(anyhow::anyhow!(
                            "connect_blocking() reaches a local socket (the ww hub, IPC) on a multi-thread tokio runtime only"
                        ));
                    }
                    rt.block_on(try_select_local_socket(&config, &mut cmd_rx, false))?
                }
            };
            match selected {
                Selected::Device {
                    handle,
                    info,
                    via_hub,
                } => {
                    let key = device_key(&iface_kind, &info, &config);
                    let device_api_info = try_connect_blocking(&cmd_tx, handle, &config)
                        .map_err(|e| connect_failed(info, e))?;
                    #[cfg(feature = "clock_sync")]
                    let clock = ClockStart::new(&config, key, watch::channel(via_hub).1);
                    #[cfg(not(feature = "clock_sync"))]
                    let _ = (key, via_hub);
                    #[allow(unused_mut)]
                    let mut commander = create_commander_blocking(config, cmd_tx, device_api_info);
                    #[cfg(feature = "clock_sync")]
                    clock.start(&mut commander);
                    return Ok(T::from_cmd(commander));
                }
                Selected::NotFound { unmatched: u } => unmatched.extend(u),
            }
        }

        Err(not_found(&config, unmatched))
    }

    pub fn connect_promise(self) {
        todo!()
    }

    /// Try to connect to a selected device immediately, but if it fails or there are no devices, keep trying.
    /// Even when connected and then disconnected or errored out, try again, until commanded to stop and exit.
    /// Commander and all open streams will be kept active.
    ///
    /// NOTE: not implemented yet
    pub fn connect_keep_trying(self) {
        // figure out how to handle Commander.connected_device if connected to a different device, reject by serial?
        todo!()
    }
}

/// What a new connection's own clock steering needs (CLT-46), taken from its config before the Commander is made.
#[cfg(feature = "clock_sync")]
struct ClockStart {
    options: Option<crate::clock_sync::Options>,
    meta: Option<Box<ww_link::ClientMetaOwned>>,
    key: Option<String>,
    via_hub: watch::Receiver<bool>,
}

#[cfg(feature = "clock_sync")]
impl ClockStart {
    fn new(config: &ValidatedConfig, key: Option<String>, via_hub: watch::Receiver<bool>) -> Self {
        ClockStart {
            options: config.clock_sync.clone(),
            meta: config.client_meta.clone(),
            key,
            via_hub,
        }
    }

    /// Starts the steering on `commander` when the config has it on.
    fn start(self, commander: &mut Commander) {
        let Some(options) = self.options else {
            return;
        };
        let bundle = commander.device_introspect().map(|b| b.api_bundle.clone());
        commander.start_clock_sync(crate::clock_sync::Start {
            options,
            bundle,
            key: self.key,
            meta: self.meta,
            via_hub: self.via_hub,
        });
    }
}

/// What a connection attempt left behind, see [connect_to].
pub(crate) struct Attempt {
    /// The device's API and the receiver of the event loop's residual, or why the attempt failed.
    pub(crate) result: Result<(DeviceApiInfo, oneshot::Receiver<EventLoopResidual>)>,
    /// Set when an event loop was started and failed to connect: the receiver of its residual.
    pub(crate) started: Option<oneshot::Receiver<EventLoopResidual>>,
    /// The device was reached through the ww hub
    pub(crate) via_hub: bool,
    /// The device's key as the holders registry and the hub name it (`usb:..`, `rtt:..`), `None` on other transports
    #[cfg_attr(not(feature = "clock_sync"), allow(dead_code))]
    pub(crate) key: Option<String>,
}

impl Attempt {
    fn failed(e: Error) -> Self {
        Attempt {
            result: Err(e),
            started: None,
            via_hub: false,
            key: None,
        }
    }
}

/// The key of a device `kind` selected (see [Attempt::key]).
fn device_key(
    kind: &InterfaceKind,
    info: &crate::DeviceInfo,
    config: &ValidatedConfig,
) -> Option<String> {
    use crate::config::ConfigPiece;
    match kind {
        InterfaceKind::Usb => crate::hub::key_of(info),
        InterfaceKind::Rtt => {
            let chip = config.pieces.iter().rev().find_map(|p| match p {
                ConfigPiece::Rtt { target, .. } => Some(target.as_str()),
                _ => None,
            })?;
            Some(crate::holders::key_for(crate::holders::DeviceRef::Rtt {
                probe_serial: info.serials.first().map(String::as_str).unwrap_or(""),
                chip,
            }))
        }
        _ => None,
    }
}

/// Selects a device with the transports of `config` (in order), starts an event loop on `cmd_rx` for it and connects.
/// The loop's residual (its command channel back) comes through the returned receiver when it exits; keeping it is
/// what makes [Commander::reconnect] possible. `cmd_rx` is taken when a transport found a device.
pub(crate) async fn connect_to(
    config: &ValidatedConfig,
    cmd_tx: &mpsc::Sender<Command>,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Attempt {
    let mut unmatched = vec![];
    let kinds = match check_interfaces(config) {
        Ok(kinds) => kinds,
        Err(e) => return Attempt::failed(e),
    };
    for iface_kind in kinds {
        let selected = match iface_kind {
            #[cfg(feature = "usb")]
            InterfaceKind::Usb => crate::usb::try_connect(config, cmd_rx).await,
            #[cfg(not(feature = "usb"))]
            InterfaceKind::Usb => continue,
            InterfaceKind::InProcess => try_select_in_process(config, cmd_rx),
            InterfaceKind::Rtt => try_select_rtt_routed(config, cmd_rx).await,
            InterfaceKind::WebSocket => try_select_ws(config, cmd_rx),
            InterfaceKind::Udp => try_select_udp(config, cmd_rx),
            InterfaceKind::Ipc => try_select_local_socket(config, cmd_rx, true).await,
        };
        match selected {
            Ok(Selected::Device {
                handle,
                info,
                via_hub,
            }) => {
                let (residual_tx, residual_rx) = oneshot::channel();
                // a link through the hub that goes is reconnected by the keeper, its streams wait for it (CLT-40)
                let keep = via_hub && config.hub_reconnect.is_some();
                let key = device_key(&iface_kind, &info, config);
                return match try_connect(cmd_tx, handle, config, residual_tx, keep).await {
                    Ok(device_api_info) => Attempt {
                        result: Ok((device_api_info, residual_rx)),
                        started: None,
                        via_hub,
                        key,
                    },
                    Err(e) => Attempt {
                        result: Err(connect_failed(info, e)),
                        started: Some(residual_rx),
                        via_hub,
                        key,
                    },
                };
            }
            Ok(Selected::NotFound { unmatched: u }) => unmatched.extend(u),
            Err(e) => return Attempt::failed(e),
        }
    }
    Attempt::failed(not_found(config, unmatched))
}

/// What [Commander::reconnect] needs: the configuration the Commander was connected with, and the keeper of the
/// event loop's command channel ([Keeper]).
pub(crate) struct Reconnect {
    config: ValidatedConfig,
    ctl: mpsc::UnboundedSender<Ctl>,
    /// [Commander::connection_events]
    pub(crate) events: broadcast::Sender<ConnectionEvent>,
    /// The link (the last one, while none runs) goes through the ww hub; changes with a reconnect
    #[cfg_attr(not(feature = "clock_sync"), allow(dead_code))]
    pub(crate) via_hub: watch::Receiver<bool>,
}

/// From [Reconnect] to its [Keeper].
enum Ctl {
    /// Hand the command channel over for a connection attempt, if no event loop runs on it
    Take(oneshot::Sender<Result<mpsc::Receiver<Command>, crate::Error>>),
    /// The attempt started an event loop on the channel, its residual comes through this receiver
    Running {
        residual: oneshot::Receiver<EventLoopResidual>,
        via_hub: bool,
    },
    /// The attempt failed before an event loop took the channel: here it is back
    Back(mpsc::Receiver<Command>),
    /// The attempt failed after it took the command channel: nothing to connect on any more
    Lost,
    /// Reconnect by itself when a link through the hub goes (CLT-40), with this Commander's streams
    Supervise(Box<WeakCommander>),
}

/// Where the command channel is, as the [Keeper] sees it.
enum Keeping {
    /// An event loop runs on it, its residual (with the channel) comes when it exits
    Running(oneshot::Receiver<EventLoopResidual>),
    /// No loop: commands are answered right away (CLT-36)
    Stopped(mpsc::Receiver<Command>),
    /// A reconnect has it
    Taken,
    Lost,
}

/// How long the first reconnect attempt waits after a failed one; doubles up to [MAX_BACKOFF].
const FIRST_BACKOFF: Duration = Duration::from_millis(10);
const MAX_BACKOFF: Duration = Duration::from_secs(1);

/// A link through the hub that went and is being reconnected (CLT-40).
struct Gap {
    since: Instant,
    attempts: u32,
    next_at: Instant,
    backoff: Duration,
    /// Without a link until then: give up. Moved on while the device is lent.
    give_up_at: Instant,
}

impl Reconnect {
    /// Starts the [Keeper] for the event loop whose residual comes through `residual`.
    fn new(
        config: ValidatedConfig,
        residual: oneshot::Receiver<EventLoopResidual>,
        via_hub: bool,
    ) -> Arc<Self> {
        let (ctl, ctl_rx) = mpsc::unbounded_channel();
        let (events, _) = broadcast::channel(64);
        let (via_hub, via_hub_rx) = watch::channel(via_hub);
        let keeper = Keeper {
            ctl: ctl_rx,
            config: config.clone(),
            events: events.clone(),
            commander: None,
            via_hub,
            gap: None,
        };
        tokio::spawn(keeper.run(residual));
        Arc::new(Reconnect {
            config,
            ctl,
            events,
            via_hub: via_hub_rx,
        })
    }

    /// From now on, a link through the hub that goes is connected again by the keeper, which resumes `commander`'s
    /// streams (CLT-40, [ClientConfig::hub_reconnect]).
    fn supervise(&self, commander: WeakCommander) {
        _ = self.ctl.send(Ctl::Supervise(Box::new(commander)));
    }

    /// Connects again with the configuration of the first connection, on the command channel of the event loop that
    /// exited (after `disconnect()`, or a link that went down).
    pub(crate) async fn run(
        &self,
        cmd_tx: &mpsc::Sender<Command>,
    ) -> Result<DeviceApiInfo, crate::Error> {
        let lost =
            || crate::Error::User("the event loop is gone, connect with a new client".into());
        let (reply_tx, reply_rx) = oneshot::channel();
        self.ctl.send(Ctl::Take(reply_tx)).map_err(|_| lost())?;
        let cmd_rx = reply_rx.await.map_err(|_| lost())??;
        let mut cmd_rx = Some(cmd_rx);
        let Attempt {
            result,
            started,
            via_hub,
            ..
        } = connect_to(&self.config, cmd_tx, &mut cmd_rx).await;
        let (next, answer) = match result {
            Ok((info, residual)) => (Ctl::Running { residual, via_hub }, Ok(info)),
            Err(e) => {
                let next = match (cmd_rx, started) {
                    (Some(cmd_rx), _) => Ctl::Back(cmd_rx),
                    (None, Some(residual)) => Ctl::Running { residual, via_hub },
                    (None, None) => Ctl::Lost,
                };
                (next, Err(crate::Error::Other(format!("{e:#}"))))
            }
        };
        _ = self.ctl.send(next);
        answer
    }
}

/// Keeps the command channel of a Commander between event loops: while none runs, every command is answered at once
/// ([Command::fail_detached]: calls fail with [Error::Disconnected](crate::Error::Disconnected), CLT-36) instead of
/// waiting in the channel for a reconnect that may never come; a reconnect takes the channel and gives it back.
/// Supervising (CLT-40), it connects again by itself when a link through the hub goes, with backoff, and resumes the
/// Commander's streams; the same for a direct link this program handed to the hub (CLT-43). Tells the app through
/// [Reconnect::events]. Ends when the Commander and its clones are gone.
struct Keeper {
    ctl: mpsc::UnboundedReceiver<Ctl>,
    config: ValidatedConfig,
    events: broadcast::Sender<ConnectionEvent>,
    commander: Option<Box<WeakCommander>>,
    /// The running (or last) event loop's link goes through the hub, see [Reconnect::via_hub]
    via_hub: watch::Sender<bool>,
    gap: Option<Gap>,
}

impl Keeper {
    async fn run(mut self, residual: oneshot::Receiver<EventLoopResidual>) {
        let mut keeping = Keeping::Running(residual);
        loop {
            keeping = match keeping {
                Keeping::Running(mut residual) => tokio::select! {
                    // first: a loop that exited before a reconnect asks for the channel has given it back by then
                    biased;
                    r = &mut residual => match r {
                        Ok(residual) => {
                            self.exited(&residual.result);
                            Keeping::Stopped(residual.cmd_rx)
                        }
                        // the loop is gone without its channel
                        Err(_) => Keeping::Lost,
                    },
                    c = self.ctl.recv() => match c {
                        Some(Ctl::Take(reply)) => {
                            _ = reply.send(Err(crate::Error::User(
                                "still connected (or the event loop is gone), disconnect().keep_streams() first".into(),
                            )));
                            Keeping::Running(residual)
                        }
                        Some(Ctl::Supervise(c)) => {
                            self.commander = Some(c);
                            Keeping::Running(residual)
                        }
                        Some(_) => Keeping::Running(residual),
                        None => return,
                    },
                },
                Keeping::Stopped(mut cmd_rx) => {
                    let next_at = self.gap.as_ref().map(|g| g.next_at);
                    tokio::select! {
                        biased;
                        c = self.ctl.recv() => match c {
                            Some(Ctl::Take(reply)) if self.gap.is_some() => {
                                _ = reply.send(Err(crate::Error::User(
                                    "reconnecting through the ww hub by itself (ClientConfig::hub_reconnect)".into(),
                                )));
                                Keeping::Stopped(cmd_rx)
                            }
                            Some(Ctl::Take(reply)) => match reply.send(Ok(cmd_rx)) {
                                Ok(()) => Keeping::Taken,
                                Err(Ok(cmd_rx)) => Keeping::Stopped(cmd_rx),
                                Err(Err(_)) => unreachable!("sent Ok"),
                            },
                            Some(Ctl::Supervise(c)) => {
                                self.commander = Some(c);
                                Keeping::Stopped(cmd_rx)
                            }
                            Some(_) => Keeping::Stopped(cmd_rx),
                            None => return,
                        },
                        cmd = cmd_rx.recv() => match cmd {
                            Some(cmd) => {
                                self.detached(&cmd);
                                cmd.fail_detached();
                                Keeping::Stopped(cmd_rx)
                            }
                            // every Commander is gone
                            None => return,
                        },
                        _ = tokio::time::sleep_until(next_at.unwrap_or_else(Instant::now).into()), if next_at.is_some() => {
                            self.attempt(cmd_rx).await
                        }
                    }
                }
                Keeping::Taken => match self.ctl.recv().await {
                    Some(Ctl::Take(reply)) => {
                        _ = reply.send(Err(crate::Error::User(
                            "a reconnect is already under way".into(),
                        )));
                        Keeping::Taken
                    }
                    Some(Ctl::Running { residual, via_hub }) => {
                        self.via_hub.send_replace(via_hub);
                        Keeping::Running(residual)
                    }
                    Some(Ctl::Back(cmd_rx)) => Keeping::Stopped(cmd_rx),
                    Some(Ctl::Lost) => Keeping::Lost,
                    Some(Ctl::Supervise(c)) => {
                        self.commander = Some(c);
                        Keeping::Taken
                    }
                    None => return,
                },
                Keeping::Lost => match self.ctl.recv().await {
                    Some(Ctl::Take(reply)) => {
                        _ = reply.send(Err(crate::Error::User(
                            "the event loop is gone, connect with a new client".into(),
                        )));
                        Keeping::Lost
                    }
                    Some(_) => Keeping::Lost,
                    None => return,
                },
            };
        }
    }

    /// The event loop exited with `result`: a link that went (not a disconnect asked for) is a gap, reconnected by
    /// itself when supervised and through the hub.
    fn exited(&mut self, result: &anyhow::Result<EventLoopExitReason>) {
        let reason = match result {
            Ok(EventLoopExitReason::DisconnectFromDevice(reason)) => {
                GapReason::from_disconnect(*reason)
            }
            Ok(EventLoopExitReason::HandedToHub) => GapReason::HandedToHub,
            Err(e) => GapReason::Lost(format!("{e:#}")),
            Ok(_) => return,
        };
        if let Some(gap) = &mut self.gap {
            // an attempt's loop that failed, or a link that went again while its streams were resumed
            debug!("link through the hub went again ({reason}), reconnecting");
            if gap.next_at < Instant::now() {
                gap.next_at = Instant::now() + gap.backoff;
            }
            return;
        }
        let via_hub = *self.via_hub.borrow();
        // a direct link handed to the hub (CLT-43) goes on through it
        let reconnecting = (via_hub || reason == GapReason::HandedToHub)
            && self.commander.is_some()
            && !matches!(
                reason,
                GapReason::Disconnect(wire_weaver::DisconnectReason::IncompatibleVersion)
            );
        info!(
            "link {}went: {reason}{}",
            if via_hub { "through the ww hub " } else { "" },
            if reconnecting { ", reconnecting" } else { "" }
        );
        if reconnecting && let Some(give_up) = self.config.hub_reconnect {
            let now = Instant::now();
            self.gap = Some(Gap {
                since: now,
                attempts: 0,
                // a hub that handed over needs a moment to have its socket in place, the others are tried at once
                next_at: now,
                backoff: FIRST_BACKOFF,
                give_up_at: now + give_up,
            });
        }
        _ = self.events.send(ConnectionEvent::Disconnected {
            reason,
            reconnecting: self.gap.is_some(),
        });
    }

    /// A command while no loop runs: a disconnect asked for ends a reconnect by itself.
    fn detached(&mut self, cmd: &Command) {
        let end_streams = match cmd {
            Command::DisconnectAndExit { .. } => true,
            Command::DisconnectKeepStreams { .. } => false,
            _ => return,
        };
        if self.gap.take().is_some() {
            debug!("disconnect asked for while reconnecting through the hub: stopped");
            if end_streams && let Some(c) = self.commander.as_ref().and_then(|c| c.upgrade()) {
                c.end_kept_streams();
            }
        }
    }

    /// One reconnect attempt.
    async fn attempt(&mut self, cmd_rx: mpsc::Receiver<Command>) -> Keeping {
        let Some(commander) = self.commander.as_ref().and_then(|c| c.upgrade()) else {
            // every Commander is gone, its streams with them
            self.gap = None;
            return Keeping::Stopped(cmd_rx);
        };
        let Some(gap) = &mut self.gap else {
            return Keeping::Stopped(cmd_rx);
        };
        gap.attempts += 1;
        let mut config = self.config.clone();
        // the device was in the hub: it is reached through a hub again, one is started if none runs
        if config.hub_mode == crate::hub::HubMode::Auto {
            config.hub_mode = crate::hub::HubMode::Prefer;
        }
        let mut cmd_rx = Some(cmd_rx);
        let Attempt {
            result,
            started,
            via_hub,
            ..
        } = connect_to(&config, commander.cmd_tx(), &mut cmd_rx).await;
        let e = match result {
            Ok((info, residual)) => {
                self.via_hub.send_replace(via_hub);
                let (since, attempts) = (gap.since, gap.attempts);
                if info.user_api_hash.no_docs != commander.connected_device.user_api_hash.no_docs {
                    self.give_up(
                        &commander,
                        "the reconnected device has another API than before: connect a new client"
                            .into(),
                    );
                    _ = commander.disconnect().asynch().await;
                    return Keeping::Running(residual);
                }
                self.gap = None;
                commander.clear_const_cache();
                let resumed = match commander.resume_streams().await {
                    Ok(n) => n,
                    Err(e) => {
                        warn!("resuming streams after a reconnect through the hub: {e}");
                        0
                    }
                };
                let gap = since.elapsed();
                info!(
                    "reconnected through the ww hub after {gap:?}, {attempts} attempts, {resumed} resumed"
                );
                _ = self.events.send(ConnectionEvent::Reconnected {
                    gap,
                    attempts,
                    resumed,
                });
                return Keeping::Running(residual);
            }
            Err(e) => e,
        };
        let refused = e.downcast_ref::<crate::Error>();
        let now = Instant::now();
        let lent = refused.is_some_and(crate::Error::is_lent);
        let fatal = matches!(
            refused,
            Some(crate::Error::HubRefused { status: 3 | 4, .. })
        );
        if lent {
            // the device comes back: lent time doesn't count
            if let Some(give_up) = self.config.hub_reconnect {
                gap.give_up_at = now + give_up;
            }
        }
        let next = match (cmd_rx, started) {
            (Some(cmd_rx), _) => Keeping::Stopped(cmd_rx),
            (None, Some(residual)) => Keeping::Running(residual),
            (None, None) => Keeping::Lost,
        };
        if fatal || now >= gap.give_up_at || matches!(next, Keeping::Lost) {
            self.give_up(&commander, format!("{e:#}"));
            return next;
        }
        let retry_in = gap.backoff;
        gap.next_at = now + retry_in;
        gap.backoff = (gap.backoff * 2).min(MAX_BACKOFF);
        debug!(
            "reconnect attempt {} through the ww hub failed: {e:#}, next in {retry_in:?}",
            gap.attempts
        );
        _ = self.events.send(ConnectionEvent::AttemptFailed {
            attempt: gap.attempts,
            error: format!("{e:#}"),
            retry_in,
        });
        next
    }

    /// No more attempts: the waiting streams end.
    fn give_up(&mut self, commander: &Commander, error: String) {
        warn!("not reconnecting through the ww hub any more: {error}");
        self.gap = None;
        commander.end_kept_streams();
        _ = self.events.send(ConnectionEvent::GaveUp { error });
    }
}

async fn try_select_rtt_routed(
    config: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Selected> {
    #[cfg(feature = "rtt")]
    return crate::rtt::try_connect_routed(config, cmd_rx).await;
    #[cfg(not(feature = "rtt"))]
    try_select_rtt(config, cmd_rx)
}

fn try_select_rtt(
    config: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Selected> {
    #[cfg(feature = "rtt")]
    return crate::rtt::try_connect(config, cmd_rx, false);
    #[cfg(not(feature = "rtt"))]
    {
        _ = (config, cmd_rx);
        Err(anyhow::anyhow!(
            "RTT selected in the client config, but wire_weaver_client is built without the `rtt` feature"
        ))
    }
}

async fn try_select_local_socket(
    config: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
    supervised: bool,
) -> Result<Selected> {
    #[cfg(feature = "local_socket")]
    return crate::local_socket::try_connect(config, cmd_rx, supervised).await;
    #[cfg(not(feature = "local_socket"))]
    {
        _ = (config, cmd_rx, supervised);
        Err(anyhow::anyhow!(
            "a local socket (IPC) selected in the client config, but wire_weaver_client is built without the `local_socket` feature"
        ))
    }
}

fn try_select_ws(
    config: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Selected> {
    #[cfg(feature = "ws")]
    return crate::ws::try_connect(config, cmd_rx);
    #[cfg(not(feature = "ws"))]
    {
        _ = (config, cmd_rx);
        Err(anyhow::anyhow!(
            "WebSocket selected in the client config, but wire_weaver_client is built without the `ws` feature"
        ))
    }
}

fn try_select_udp(
    config: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Selected> {
    #[cfg(feature = "udp")]
    return crate::udp::try_connect(config, cmd_rx);
    #[cfg(not(feature = "udp"))]
    {
        _ = (config, cmd_rx);
        Err(anyhow::anyhow!(
            "UDP selected in the client config, but wire_weaver_client is built without the `udp` feature"
        ))
    }
}

fn try_select_in_process(
    config: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Selected> {
    #[cfg(feature = "in_process")]
    return crate::in_process::try_connect(config, cmd_rx);
    #[cfg(not(feature = "in_process"))]
    {
        _ = (config, cmd_rx);
        Err(anyhow::anyhow!(
            "In-process device selected in the client config, but wire_weaver_client is built without the `in_process` feature"
        ))
    }
}

async fn try_connect(
    cmd_tx: &mpsc::Sender<Command>,
    handle: Box<dyn Any + Send>,
    config: &ValidatedConfig,
    residual_tx: oneshot::Sender<EventLoopResidual>,
    keep_streams_on_loss: bool,
) -> Result<DeviceApiInfo> {
    let (connected_tx, connected_rx) = oneshot::channel();
    cmd_tx
        .send(Command::Connect {
            handle,
            client_version: config.client_version.clone(),
            max_seq: config.max_seq,
            link_setup_timeout_ms: config.link_setup_timeout_ms,
            tap: config.tap.clone(),
            client_meta: config.client_meta.clone(),
            keep_streams_on_loss,
            connected_tx: Some(connected_tx),
            failed_tx: Some(residual_tx),
        })
        .await
        .map_err(|_| crate::Error::EventLoopNotRunning)?;
    let info = connected_rx.await?;
    let device_api_info = info.result?;
    Ok(device_api_info)
}

fn try_connect_blocking(
    cmd_tx: &mpsc::Sender<Command>,
    handle: Box<dyn Any + Send>,
    config: &ValidatedConfig,
) -> Result<DeviceApiInfo> {
    let (connected_tx, connected_rx) = oneshot::channel();
    cmd_tx
        .blocking_send(Command::Connect {
            handle,
            client_version: config.client_version.clone(),
            max_seq: config.max_seq,
            link_setup_timeout_ms: config.link_setup_timeout_ms,
            tap: config.tap.clone(),
            client_meta: config.client_meta.clone(),
            keep_streams_on_loss: false,
            connected_tx: Some(connected_tx),
            failed_tx: None,
        })
        .map_err(|_| crate::Error::EventLoopNotRunning)?;
    let info = connected_rx.blocking_recv()?;
    let device_api_info = info.result?;
    Ok(device_api_info)
}

fn create_commander_inner(
    config: ValidatedConfig,
    cmd_tx: mpsc::Sender<Command>,
    device_api_info: DeviceApiInfo,
) -> Commander {
    let mut commander = Commander::new(cmd_tx);
    commander.set_local_timeout(config.default_timeout);
    commander.traits_only = config.traits_only;
    commander.set_stream_queue_bytes(config.stream_queue_bytes);
    commander.set_stream_compression(config.stream_compression);
    if let Some(introspect_client) = config.introspect_client {
        if introspect_client.api_hash.no_docs == device_api_info.user_api_hash.no_docs {
            // connected device API is exactly the same as client, no need to download or look for cached one
            commander.set_device_introspect(introspect_client.clone());
        }
        commander.set_client_introspect(introspect_client);
    }
    commander.connected_device = device_api_info;
    commander
}

/// NOTE: keep in sync with [create_commander_blocking]
async fn create_commander(
    config: ValidatedConfig,
    cmd_tx: mpsc::Sender<Command>,
    device_api_info: DeviceApiInfo,
) -> Commander {
    let device_api_hash = device_api_info.user_api_hash.clone();
    let mut commander = create_commander_inner(config, cmd_tx, device_api_info);
    if commander.device_introspect().is_none() && !commander.traits_only {
        match commander.introspect().get_as_sent().await {
            Ok(Some((sent_api_bundle, sent_size))) => {
                match IntrospectBundle::from_sent(sent_api_bundle, device_api_hash, sent_size) {
                    Ok(bundle) => commander.set_device_introspect(bundle),
                    Err(e) => warn!("Device introspection data is not usable: {e:#}"),
                }
            }
            Ok(None) => debug!("device has introspection disabled and its API is not in the cache"),
            Err(e) => warn!("Failed to get device introspection data: {e:#}"),
        }
    }
    commander.resolve_api_match();
    commander
}

/// NOTE: keep in sync with [create_commander]
fn create_commander_blocking(
    config: ValidatedConfig,
    cmd_tx: mpsc::Sender<Command>,
    device_api_info: DeviceApiInfo,
) -> Commander {
    let device_api_hash = device_api_info.user_api_hash.clone();
    let mut commander = create_commander_inner(config, cmd_tx, device_api_info);
    if commander.device_introspect().is_none() && !commander.traits_only {
        match commander.introspect().get_as_sent_blocking() {
            Ok(Some((sent_api_bundle, sent_size))) => {
                match IntrospectBundle::from_sent(sent_api_bundle, device_api_hash, sent_size) {
                    Ok(bundle) => commander.set_device_introspect(bundle),
                    Err(e) => warn!("Device introspection data is not usable: {e:#}"),
                }
            }
            Ok(None) => debug!("device has introspection disabled and its API is not in the cache"),
            Err(e) => warn!("Failed to get device introspection data: {e:#}"),
        }
    }
    commander.resolve_api_match();
    commander
}

fn check_interfaces(config: &ValidatedConfig) -> Result<Vec<InterfaceKind>> {
    let interfaces = config.interfaces();
    if interfaces.is_empty() {
        return Err(crate::Error::NoTransportSelected.into());
    }
    Ok(interfaces)
}

fn not_found(config: &ValidatedConfig, unmatched: Vec<crate::DeviceInfo>) -> Error {
    crate::Error::DeviceNotFound {
        filters: config.describe_filters(),
        unmatched,
    }
    .into()
}

// Event loop is already started for this device, so there is no point in trying other ones
fn connect_failed(device: Box<crate::DeviceInfo>, e: Error) -> Error {
    crate::Error::ConnectFailed {
        device,
        reason: format!("{e:#}"),
    }
    .into()
}

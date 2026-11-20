use std::{any::Any, marker::PhantomData};

use crate::config::InterfaceKind;
use crate::event_loop::command::EventLoopExitReason;
use crate::event_loop::commander::WeakCommander;
use crate::event_loop::transport::Selected;
use crate::{
    Backoff, Commander, ConnectionEvent, ConnectionState, GapReason, WwClient,
    config::{ClientConfig, IntrospectBundle, ValidatedConfig},
    device_info::DeviceApiInfo,
    event_loop::command::{Command, EventLoopResidual},
};
use anyhow::{Error, Result};
use std::sync::Arc;
use std::time::Duration;
// tokio's clock: a test on the paused clock moves the keeper's backoff and give-up times
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio::time::Instant;
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
        self.connect_with(vec![]).await
    }

    /// Connects going on with another program's live link (CLT-44, the ww hub adopting a direct holder's link): no
    /// link setup, the link is up as soon as the device is open. The config must name the same device.
    /// Answers to the other program's requests in flight go to `sink` from the first frame read on.
    pub async fn adopt(
        self,
        adopted: crate::event_loop::command::Adopted,
        sink: Box<dyn crate::event_loop::command::ForwardSink>,
    ) -> Result<T> {
        self.connect_with(vec![
            Command::ForwardSink { sink },
            Command::Adopt {
                adopted: Box::new(adopted),
            },
        ])
        .await
    }

    async fn connect_with(self, first: Vec<Command>) -> Result<T> {
        let config = self.config.validate()?;
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(config.cmd_queue_size);
        for cmd in first {
            // queued: the event loop takes them before the Connect
            cmd_tx
                .try_send(cmd)
                .map_err(|_| crate::Error::EventLoopNotRunning)?;
        }
        let mut cmd_rx = Some(cmd_rx);
        let attempt = connect_to(&config, &cmd_tx, &mut cmd_rx).await;
        let (device_api_info, residual_rx) = attempt.result?;
        let reconnect = Reconnect::new(
            config.clone(),
            residual_rx,
            attempt.via_hub,
            attempt.address,
        );
        let supervise = config.hub_reconnect.is_some();
        #[cfg(feature = "clock_sync")]
        let clock = ClockStart::new(&config, attempt.key, reconnect.via_hub.clone());
        let mut commander = create_commander(config, cmd_tx, device_api_info).await?;
        if supervise {
            reconnect.supervise(commander.downgrade());
        }
        commander.set_reconnect(reconnect);
        #[cfg(feature = "clock_sync")]
        clock.start(&mut commander);
        Ok(T::try_from_cmd(commander)?)
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
                    #[allow(unused_mut, reason = "mutated only with the `clock_sync` feature")]
                    let mut commander = create_commander_blocking(config, cmd_tx, device_api_info)?;
                    #[cfg(feature = "clock_sync")]
                    clock.start(&mut commander);
                    return Ok(T::try_from_cmd(commander)?);
                }
                Selected::NotFound { unmatched: u } => unmatched.extend(u),
            }
        }

        Err(not_found(&config, unmatched))
    }

    /// Makes the client at once and connects in the background, again whenever the link goes (CLT-75): for a device
    /// that comes up later, or a link that drops and comes back (a phone on Wi-Fi, a device that reboots). Each
    /// attempt tries the config's addresses in order ([ClientConfig::websocket_urls]), the first that answers wins;
    /// between attempts the client waits as [ClientConfig::keep_trying] says. Until it is connected, calls fail at
    /// once with [Error::Disconnected](crate::Error::Disconnected), and so do calls in flight when the link goes:
    /// none is sent again by itself. After each reconnect, counted and reliable streams, observed levels and sinks go
    /// on ([Commander::resume_streams]). [Commander::connection_events] and [Commander::connection_state] tell the
    /// app; `disconnect()` stops trying.
    ///
    /// Until the first connect the client assumes the device runs its own API (the first wire line of a client that
    /// speaks several); the first connect learns the device's API as `connect()` does. A device on another wire line
    /// then, or one with another API on a later reconnect, ends the trying with
    /// [ConnectionEvent::GaveUp](crate::ConnectionEvent::GaveUp): make a new client for it. Runs on the caller's
    /// tokio runtime.
    pub fn connect_keep_trying(self) -> Result<T> {
        if tokio::runtime::Handle::try_current().is_err() {
            return Err(anyhow::anyhow!(
                "connect_keep_trying() runs on a tokio runtime"
            ));
        }
        let config = self.config.validate()?;
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(config.cmd_queue_size);
        let expected = expected_device(&config);
        let mut commander = create_commander_inner(config.clone(), cmd_tx, expected);
        commander.resolve_api_match()?;
        commander.expect_learned();
        let mut config = config;
        config.keeps_trying = true;
        let reconnect = Reconnect::keep_trying(config.clone(), cmd_rx);
        #[cfg(feature = "clock_sync")]
        if let Some(clock) =
            ClockStart::new(&config, None, reconnect.via_hub.clone()).prepare(&mut commander)
        {
            _ = reconnect.ctl.send(Ctl::Clock(Box::new(clock)));
        }
        reconnect.supervise(commander.downgrade());
        commander.set_reconnect(reconnect);
        Ok(T::try_from_cmd(commander)?)
    }
}

/// What a keep-trying connection's client expects before its first connect (CLT-75): a device running the client's
/// own API, nothing known for a dynamic client.
fn expected_device(config: &ValidatedConfig) -> DeviceApiInfo {
    let mut info = DeviceApiInfo::empty();
    if !config.client_version.is_dynamic() {
        info.user_api_version = config.client_version.version.clone();
        info.user_api_wire = config.client_version.wire;
        if let Some(bundle) = &config.introspect_client {
            info.user_api_hash = bundle.api_hash.clone();
        }
    }
    info
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

    /// Prepares the steering of a keep-trying connection (CLT-75), which [PendingClock::run] starts on its first
    /// connect: `None` when the config has it off.
    fn prepare(self, commander: &mut Commander) -> Option<PendingClock> {
        self.options.as_ref()?;
        let senders = commander.prepare_clock_sync()?;
        Some(PendingClock {
            start: self,
            senders,
        })
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

/// The clock steering of a keep-trying connection, waiting for its first connect (CLT-75).
#[cfg(feature = "clock_sync")]
struct PendingClock {
    start: ClockStart,
    senders: crate::clock_sync::auto::Senders,
}

#[cfg(feature = "clock_sync")]
impl PendingClock {
    /// Starts the steering, now that `commander` knows the device's API.
    fn run(self, commander: &Commander) {
        let Some(options) = self.start.options else {
            return;
        };
        let bundle = commander.device_introspect().map(|b| b.api_bundle.clone());
        commander.run_clock_sync(
            self.senders,
            crate::clock_sync::Start {
                options,
                bundle,
                key: self.start.key,
                meta: self.start.meta,
                via_hub: self.start.via_hub,
            },
        );
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
    #[cfg_attr(
        not(feature = "clock_sync"),
        allow(dead_code, reason = "only used with the `clock_sync` feature")
    )]
    pub(crate) key: Option<String>,
    /// Where the device was reached, as [DeviceInfo::location](crate::DeviceInfo::location) names it; empty when
    /// nothing was selected
    pub(crate) address: String,
}

impl Attempt {
    fn failed(e: Error) -> Self {
        Attempt {
            result: Err(e),
            started: None,
            via_hub: false,
            key: None,
            address: String::new(),
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

/// Connects as [connect_one] does, to the config's addresses in order ([ValidatedConfig::alternatives], CLT-75): the
/// first that connects wins. A failed address gives the command channel back before the next is tried; the error of
/// the last one names every failure.
pub(crate) async fn connect_to(
    config: &ValidatedConfig,
    cmd_tx: &mpsc::Sender<Command>,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Attempt {
    let alternatives = config.alternatives();
    let mut failed = Vec::new();
    for (i, alternative) in alternatives.iter().enumerate() {
        let mut attempt = connect_one(alternative, cmd_tx, cmd_rx).await;
        let last = i + 1 == alternatives.len();
        if attempt.result.is_ok() || last {
            if let Err(e) = &attempt.result
                && !failed.is_empty()
            {
                failed.push(format!("{e:#}"));
                attempt.result = Err(anyhow::anyhow!("{}", failed.join("; ")));
            }
            return attempt;
        }
        if let Some(residual) = attempt.started.take() {
            match residual.await {
                Ok(residual) => *cmd_rx = Some(residual.cmd_rx),
                // the loop is gone without its channel: nothing to try the next address on
                Err(_) => return attempt,
            }
        }
        if cmd_rx.is_none() {
            return attempt;
        }
        if let Err(e) = &attempt.result {
            failed.push(format!("{e:#}"));
        }
    }
    unreachable!("the last alternative returns, and alternatives() has at least one")
}

/// Selects a device with the transports of `config` (in order), starts an event loop on `cmd_rx` for it and connects.
/// The loop's residual (its command channel back) comes through the returned receiver when it exits; keeping it is
/// what makes [Commander::reconnect] possible. `cmd_rx` is taken when a transport found a device.
async fn connect_one(
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
                // a link through the hub that goes is reconnected by the keeper, its streams wait for it (CLT-40), as
                // does every link of a keep-trying connection (CLT-75)
                let keep = config.keeps_trying || (via_hub && config.hub_reconnect.is_some());
                let key = device_key(&iface_kind, &info, config);
                let address = info.location.clone();
                return match try_connect(cmd_tx, handle, config, residual_tx, keep).await {
                    Ok(device_api_info) => Attempt {
                        result: Ok((device_api_info, residual_rx)),
                        started: None,
                        via_hub,
                        key,
                        address,
                    },
                    Err(e) => Attempt {
                        result: Err(connect_failed(info, e)),
                        started: Some(residual_rx),
                        via_hub,
                        key,
                        address,
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
    /// [Commander::connection_state]
    pub(crate) state: watch::Receiver<ConnectionState>,
    /// The link (the last one, while none runs) goes through the ww hub; changes with a reconnect
    #[cfg_attr(
        not(feature = "clock_sync"),
        allow(dead_code, reason = "only used with the `clock_sync` feature")
    )]
    pub(crate) via_hub: watch::Receiver<bool>,
}

/// From [Reconnect] to its [Keeper].
enum Ctl {
    /// Hand the command channel over for a connection attempt, if no event loop runs on it
    Take(oneshot::Sender<Result<mpsc::Receiver<Command>, crate::Error>>),
    /// The attempt started an event loop on the channel, its residual comes through this receiver; `connected` names
    /// the address when it connected
    Running {
        residual: oneshot::Receiver<EventLoopResidual>,
        via_hub: bool,
        connected: Option<String>,
    },
    /// The attempt failed before an event loop took the channel: here it is back
    Back(mpsc::Receiver<Command>),
    /// The attempt failed after it took the command channel: nothing to connect on any more
    Lost,
    /// Reconnect by itself when a link through the hub goes (CLT-40), or any link of a keep-trying connection
    /// (CLT-75), with this Commander's streams
    Supervise(Box<WeakCommander>),
    /// The clock steering a keep-trying connection starts on its first connect
    #[cfg(feature = "clock_sync")]
    Clock(Box<PendingClock>),
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

/// A link that went and is being reconnected (CLT-40), or a keep-trying connection's first connect (CLT-75).
struct Gap {
    since: Instant,
    attempts: u32,
    next_at: Instant,
    /// The wait after the next failed attempt
    backoff: Duration,
    /// Without a link until then: give up; `None`: never. Moved on while the device is lent.
    give_up_at: Option<Instant>,
}

impl Gap {
    fn new(backoff: Backoff, give_up: Option<Duration>) -> Self {
        let now = Instant::now();
        Gap {
            since: now,
            attempts: 0,
            // a hub that handed over needs a moment to have its socket in place, the others are tried at once
            next_at: now,
            backoff: backoff.first,
            give_up_at: give_up.map(|g| now + g),
        }
    }
}

impl Reconnect {
    /// Starts the [Keeper] for the event loop whose residual comes through `residual`, connected at `address`.
    fn new(
        config: ValidatedConfig,
        residual: oneshot::Receiver<EventLoopResidual>,
        via_hub: bool,
        address: String,
    ) -> Arc<Self> {
        Self::start(
            config,
            Keeping::Running(residual),
            via_hub,
            ConnectionState::Connected { address },
            None,
        )
    }

    /// Starts the [Keeper] of a keep-trying connection (CLT-75), which makes its first connect.
    fn keep_trying(config: ValidatedConfig, cmd_rx: mpsc::Receiver<Command>) -> Arc<Self> {
        let gap = Gap::new(config.keep_trying.backoff, config.keep_trying.give_up);
        Self::start(
            config,
            Keeping::Stopped(cmd_rx),
            false,
            ConnectionState::Connecting { attempt: 0 },
            Some(gap),
        )
    }

    fn start(
        config: ValidatedConfig,
        keeping: Keeping,
        via_hub: bool,
        state: ConnectionState,
        gap: Option<Gap>,
    ) -> Arc<Self> {
        let (ctl, ctl_rx) = mpsc::unbounded_channel();
        let (events, _) = broadcast::channel(EVENTS_QUEUE_LEN);
        let (via_hub, via_hub_rx) = watch::channel(via_hub);
        let (state, state_rx) = watch::channel(state);
        let keeper = Keeper {
            ctl: ctl_rx,
            config: config.clone(),
            events: events.clone(),
            state,
            commander: None,
            via_hub,
            gap,
            #[cfg(feature = "clock_sync")]
            clock: None,
        };
        tokio::spawn(keeper.run(keeping));
        Arc::new(Reconnect {
            config,
            ctl,
            events,
            state: state_rx,
            via_hub: via_hub_rx,
        })
    }

    /// From now on, a link that goes is connected again by the keeper, which resumes `commander`'s streams: a link
    /// through the hub (CLT-40, [ClientConfig::hub_reconnect]), any link of a keep-trying connection (CLT-75).
    fn supervise(&self, commander: WeakCommander) {
        _ = self.ctl.send(Ctl::Supervise(Box::new(commander)));
    }

    /// The configuration the Commander was connected with, for [Commander::reconnect_as](crate::Commander::reconnect_as)
    /// to rebuild one of the device's new line (CLT-63).
    pub(crate) fn config(&self) -> &ValidatedConfig {
        &self.config
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
            address,
            ..
        } = connect_to(&self.config, cmd_tx, &mut cmd_rx).await;
        let (next, answer) = match result {
            Ok((info, residual)) => (
                Ctl::Running {
                    residual,
                    via_hub,
                    connected: Some(address),
                },
                Ok(info),
            ),
            Err(e) => {
                let next = match (cmd_rx, started) {
                    (Some(cmd_rx), _) => Ctl::Back(cmd_rx),
                    (None, Some(residual)) => Ctl::Running {
                        residual,
                        via_hub,
                        connected: None,
                    },
                    (None, None) => Ctl::Lost,
                };
                (next, Err(crate::Error::Other(format!("{e:#}"))))
            }
        };
        _ = self.ctl.send(next);
        answer
    }
}

/// Events a [Commander::connection_events] receiver may lag behind before it misses some.
const EVENTS_QUEUE_LEN: usize = 64;

/// Keeps the command channel of a Commander between event loops: while none runs, every command is answered at once
/// ([Command::fail_detached]: calls fail with [Error::Disconnected](crate::Error::Disconnected), CLT-36) instead of
/// waiting in the channel for a reconnect that may never come; a reconnect takes the channel and gives it back.
/// Supervising, it connects again by itself, with backoff, and resumes the Commander's streams: when a link through
/// the hub goes (CLT-40), a direct link this program handed to the hub (CLT-43), and every link of a keep-trying
/// connection, whose first connect it makes too (CLT-75). Tells the app through [Reconnect::events] and
/// [Reconnect::state]. Ends when the Commander and its clones are gone.
struct Keeper {
    ctl: mpsc::UnboundedReceiver<Ctl>,
    config: ValidatedConfig,
    events: broadcast::Sender<ConnectionEvent>,
    state: watch::Sender<ConnectionState>,
    commander: Option<Box<WeakCommander>>,
    /// The running (or last) event loop's link goes through the hub, see [Reconnect::via_hub]
    via_hub: watch::Sender<bool>,
    gap: Option<Gap>,
    /// Started on a keep-trying connection's first connect
    #[cfg(feature = "clock_sync")]
    clock: Option<Box<PendingClock>>,
}

impl Keeper {
    async fn run(mut self, mut keeping: Keeping) {
        loop {
            keeping = match keeping {
                Keeping::Running(mut residual) => tokio::select! {
                    // first: a loop that exited before a reconnect asks for the channel has given it back by then
                    biased;
                    r = &mut residual => match r {
                        Ok(residual) => {
                            if residual.via_hub {
                                self.via_hub.send_replace(true);
                            }
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
                        Some(c) => {
                            self.setting(c);
                            Keeping::Running(residual)
                        }
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
                                    "reconnecting by itself (through the ww hub, or a keep-trying connection)".into(),
                                )));
                                Keeping::Stopped(cmd_rx)
                            }
                            Some(Ctl::Take(reply)) => match reply.send(Ok(cmd_rx)) {
                                Ok(()) => Keeping::Taken,
                                Err(Ok(cmd_rx)) => Keeping::Stopped(cmd_rx),
                                Err(Err(_)) => unreachable!("sent Ok"),
                            },
                            Some(c) => {
                                self.setting(c);
                                Keeping::Stopped(cmd_rx)
                            }
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
                        _ = tokio::time::sleep_until(next_at.unwrap_or_else(Instant::now)), if next_at.is_some() => {
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
                    Some(Ctl::Running {
                        residual,
                        via_hub,
                        connected,
                    }) => {
                        self.via_hub.send_replace(via_hub);
                        if let Some(address) = connected {
                            self.state
                                .send_replace(ConnectionState::Connected { address });
                        }
                        Keeping::Running(residual)
                    }
                    Some(Ctl::Back(cmd_rx)) => Keeping::Stopped(cmd_rx),
                    Some(Ctl::Lost) => Keeping::Lost,
                    Some(c) => {
                        self.setting(c);
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

    /// A [Ctl] that sets what the keeper works with, in any state.
    fn setting(&mut self, c: Ctl) {
        match c {
            Ctl::Supervise(c) => self.commander = Some(c),
            #[cfg(feature = "clock_sync")]
            Ctl::Clock(clock) => self.clock = Some(clock),
            _ => {}
        }
    }

    /// How long a gap may last before the keeper gives up: `None` when this connection doesn't reconnect by itself,
    /// `Some(None)` for never.
    fn give_up_after(&self) -> Option<Option<Duration>> {
        if self.config.keeps_trying {
            Some(self.config.keep_trying.give_up)
        } else {
            self.config.hub_reconnect.map(Some)
        }
    }

    fn backoff(&self) -> Backoff {
        if self.config.keeps_trying {
            self.config.keep_trying.backoff
        } else {
            Backoff::HUB
        }
    }

    /// The event loop exited with `result`: a link that went (not a disconnect asked for) is a gap, reconnected by
    /// itself when supervised and through the hub, or on a keep-trying connection.
    fn exited(&mut self, result: &anyhow::Result<EventLoopExitReason>) {
        let reason = match result {
            Ok(EventLoopExitReason::DisconnectFromDevice(reason)) => {
                GapReason::from_disconnect(*reason)
            }
            Ok(EventLoopExitReason::HandedToHub) => GapReason::HandedToHub,
            Err(e) => GapReason::Lost(format!("{e:#}")),
            Ok(_) => {
                if self.gap.is_none() {
                    self.state.send_replace(ConnectionState::Disconnected);
                }
                return;
            }
        };
        if let Some(gap) = &mut self.gap {
            // an attempt's loop that failed, or a link that went again while its streams were resumed
            debug!("link went again ({reason}), reconnecting");
            if gap.next_at < Instant::now() {
                gap.next_at = Instant::now() + gap.backoff;
            }
            return;
        }
        let via_hub = *self.via_hub.borrow();
        // a direct link handed to the hub (CLT-43) goes on through it
        let reconnecting =
            (via_hub || reason == GapReason::HandedToHub || self.config.keeps_trying)
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
        if reconnecting && let Some(give_up) = self.give_up_after() {
            self.gap = Some(Gap::new(self.backoff(), give_up));
        }
        self.state.send_replace(match self.gap {
            Some(_) => ConnectionState::Connecting { attempt: 0 },
            None => ConnectionState::Disconnected,
        });
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
            debug!("disconnect asked for while reconnecting: stopped");
            self.state.send_replace(ConnectionState::Disconnected);
            if end_streams && let Some(c) = self.commander.as_ref().and_then(|c| c.upgrade()) {
                c.end_kept_streams();
            }
        }
    }

    /// One reconnect attempt, or a keep-trying connection's first connect.
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
        let attempt = gap.attempts;
        let mut config = self.config.clone();
        // the device was in the hub: it is reached through a hub again, one is started if none runs
        if !config.keeps_trying && config.hub_mode == crate::hub::HubMode::Auto {
            config.hub_mode = crate::hub::HubMode::Prefer;
        }
        self.state
            .send_replace(ConnectionState::Connecting { attempt });
        if config.keeps_trying {
            _ = self.events.send(ConnectionEvent::Connecting { attempt });
        }
        let mut cmd_rx = Some(cmd_rx);
        let Attempt {
            result,
            started,
            via_hub,
            address,
            ..
        } = connect_to(&config, commander.cmd_tx(), &mut cmd_rx).await;
        let e = match result {
            Ok((info, residual)) => {
                self.via_hub.send_replace(via_hub);
                return if commander.has_learned() {
                    self.reconnected(commander, info, address).await;
                    Keeping::Running(residual)
                } else {
                    self.first_connect(commander, info, address).await;
                    Keeping::Running(residual)
                };
            }
            Err(e) => e,
        };
        self.failed(&commander, e, cmd_rx, started)
    }

    /// An attempt connected again after a gap: on the same API, the Commander's streams go on.
    async fn reconnected(&mut self, commander: Commander, info: DeviceApiInfo, address: String) {
        let Some(gap) = self.gap.take() else {
            return;
        };
        commander.note_docs_updated(&info);
        if info.user_api_hash.no_docs != commander.device_api_info().user_api_hash.no_docs {
            // CLT-63: names both wire lines when the device changed line (a DFU); this loop has no `T` to pick a
            // client with, unlike Commander::reconnect_as, so it always gives up here
            self.give_up(
                &commander,
                commander.reconnected_elsewhere_error(info).to_string(),
            );
            _ = commander.disconnect().asynch().await;
            return;
        }
        commander.clear_const_cache();
        let resumed = match commander.resume_streams().await {
            Ok(n) => n,
            Err(e) => {
                warn!("resuming streams after a reconnect: {e}");
                0
            }
        };
        let (attempts, gap) = (gap.attempts, gap.since.elapsed());
        info!("reconnected to {address} after {gap:?}, {attempts} attempts, {resumed} resumed");
        self.state
            .send_replace(ConnectionState::Connected { address });
        _ = self.events.send(ConnectionEvent::Reconnected {
            gap,
            attempts,
            resumed,
        });
    }

    /// A keep-trying connection's first connect (CLT-75): the Commander learns the device's API, as `connect()`
    /// would, and every clone sees it.
    async fn first_connect(
        &mut self,
        mut commander: Commander,
        info: DeviceApiInfo,
        address: String,
    ) {
        let attempts = self.gap.as_ref().map_or(0, |g| g.attempts);
        if let Err(e) = learn_first(&mut commander, info).await {
            self.give_up(&commander, e.to_string());
            _ = commander.disconnect().asynch().await;
            return;
        }
        self.gap = None;
        #[cfg(feature = "clock_sync")]
        if let Some(clock) = self.clock.take() {
            clock.run(&commander);
        }
        info!("connected to {address} after {attempts} attempts");
        self.state.send_replace(ConnectionState::Connected {
            address: address.clone(),
        });
        _ = self
            .events
            .send(ConnectionEvent::Connected { address, attempts });
    }

    /// An attempt that failed with `e`: the next one after the backoff, or no more.
    fn failed(
        &mut self,
        commander: &Commander,
        e: Error,
        cmd_rx: Option<mpsc::Receiver<Command>>,
        started: Option<oneshot::Receiver<EventLoopResidual>>,
    ) -> Keeping {
        let next = match (cmd_rx, started) {
            (Some(cmd_rx), _) => Keeping::Stopped(cmd_rx),
            (None, Some(residual)) => Keeping::Running(residual),
            (None, None) => Keeping::Lost,
        };
        let backoff = self.backoff();
        let give_up = self.give_up_after().flatten();
        let Some(gap) = &mut self.gap else {
            return next;
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
            gap.give_up_at = give_up.map(|g| now + g);
        }
        if fatal || gap.give_up_at.is_some_and(|at| now >= at) || matches!(next, Keeping::Lost) {
            self.give_up(commander, format!("{e:#}"));
            return next;
        }
        let retry_in = gap.backoff;
        gap.next_at = now + retry_in;
        gap.backoff = backoff.after(gap.backoff);
        debug!(
            "connect attempt {} failed: {e:#}, next in {retry_in:?}",
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
        warn!("not reconnecting any more: {error}");
        self.gap = None;
        commander.end_kept_streams();
        self.state.send_replace(ConnectionState::Disconnected);
        _ = self.events.send(ConnectionEvent::GaveUp { error });
    }
}

/// What a keep-trying connection's first connect (CLT-75) learns: the device's API, refused on another wire line
/// than the one the client was made for.
async fn learn_first(commander: &mut Commander, info: DeviceApiInfo) -> Result<(), crate::Error> {
    if let (Some(expected), Some(device)) = (
        commander.device_api_info().user_api_wire,
        info.user_api_wire,
    ) && !expected.same_line(&device)
    {
        return Err(commander.reconnected_elsewhere_error(info));
    }
    commander.set_connected_device(info);
    learn_device(commander).await?;
    commander.learn();
    Ok(())
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
        .send(Command::GlobalPaths(config.global_paths.clone()))
        .await
        .map_err(|_| crate::Error::EventLoopNotRunning)?;
    cmd_tx
        .send(Command::Connect {
            handle,
            client_version: config.client_version.clone(),
            max_seq: config.max_seq,
            max_message_len: config.max_message_len as u32,
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
        .blocking_send(Command::GlobalPaths(config.global_paths.clone()))
        .map_err(|_| crate::Error::EventLoopNotRunning)?;
    cmd_tx
        .blocking_send(Command::Connect {
            handle,
            client_version: config.client_version.clone(),
            max_seq: config.max_seq,
            max_message_len: config.max_message_len as u32,
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
    commander.set_default_priority(config.default_priority);
    commander.traits_only = config.traits_only;
    commander.global_paths = config.global_paths.clone();
    commander.resolver = config.resolver;
    commander.set_stream_queue_bytes(config.stream_queue_bytes);
    commander.set_stream_compression(config.stream_compression);
    // a host speaking several lines (EVO-28) has the introspection data of the device's line
    let other_line = device_api_info.user_api_wire.and_then(|device_wire| {
        config
            .other_introspect
            .into_iter()
            .find(|(wire, _)| wire.same_line(&device_wire))
    });
    let introspect_client = match other_line {
        Some((_, bundle)) => Some(bundle),
        None => config.introspect_client,
    };
    if let Some(introspect_client) = introspect_client {
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
pub(crate) async fn create_commander(
    config: ValidatedConfig,
    cmd_tx: mpsc::Sender<Command>,
    device_api_info: DeviceApiInfo,
) -> Result<Commander, crate::Error> {
    let mut commander = create_commander_inner(config, cmd_tx, device_api_info);
    learn_device(&mut commander).await?;
    Ok(commander)
}

/// The connected device's API for `commander`: its introspection data unless the client has it already, the global
/// trait paths, how the client's resources match the device's.
async fn learn_device(commander: &mut Commander) -> Result<(), crate::Error> {
    let device_api_hash = commander.device_api_info().user_api_hash.clone();
    // the device has no introspection data: known for good, unlike a failed download (SRV-53)
    let mut no_introspection = false;
    if commander.device_introspect().is_none() && !commander.traits_only {
        match commander.introspect().get_as_sent().await {
            Ok(Some((sent_api_bundle, sent_size))) => {
                match IntrospectBundle::from_sent(
                    sent_api_bundle,
                    device_api_hash.clone(),
                    sent_size,
                    commander.resolver,
                ) {
                    Ok(bundle) => commander.set_device_introspect(bundle),
                    Err(e) => warn!("Device introspection data is not usable: {e:#}"),
                }
            }
            Ok(None) => {
                debug!("device has introspection disabled and its API is not in the cache");
                no_introspection = true;
            }
            Err(e) => warn!("Failed to get device introspection data: {e:#}"),
        }
    }
    attach_global_paths(commander, device_api_hash, no_introspection);
    commander.resolve_api_match()
}

/// NOTE: keep in sync with [create_commander]
fn create_commander_blocking(
    config: ValidatedConfig,
    cmd_tx: mpsc::Sender<Command>,
    device_api_info: DeviceApiInfo,
) -> Result<Commander, crate::Error> {
    let device_api_hash = device_api_info.user_api_hash.clone();
    let mut commander = create_commander_inner(config, cmd_tx, device_api_info);
    // the device has no introspection data: known for good, unlike a failed download (SRV-53)
    let mut no_introspection = false;
    if commander.device_introspect().is_none() && !commander.traits_only {
        match commander.introspect().get_as_sent_blocking() {
            Ok(Some((sent_api_bundle, sent_size))) => {
                match IntrospectBundle::from_sent(
                    sent_api_bundle,
                    device_api_hash.clone(),
                    sent_size,
                    commander.resolver,
                ) {
                    Ok(bundle) => commander.set_device_introspect(bundle),
                    Err(e) => warn!("Device introspection data is not usable: {e:#}"),
                }
            }
            Ok(None) => {
                debug!("device has introspection disabled and its API is not in the cache");
                no_introspection = true;
            }
            Err(e) => warn!("Failed to get device introspection data: {e:#}"),
        }
    }
    attach_global_paths(&commander, device_api_hash, no_introspection);
    commander.resolve_api_match()?;
    Ok(commander)
}

/// Tell the connection's global trait paths (SRV-53) how to reach the device, and its API if the connection has it.
fn attach_global_paths(
    commander: &Commander,
    api_hash: ww_version::ApiHashPairOwned,
    no_introspection: bool,
) {
    let api = match commander.device_introspect() {
        Some(bundle) => Some(Some(bundle)),
        None if no_introspection => Some(None),
        None => None,
    };
    commander
        .global_paths
        .attach(commander.transport().downgrade(), api_hash, api);
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
        over_rtt: config.is_rtt(),
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

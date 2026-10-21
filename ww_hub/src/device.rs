//! A device the hub holds (SRV-29): one `wire_weaver_client` event loop to it, shared by every client's tunnel.
//!
//! Requests go from a client's tunnel straight to the event loop (`Command::Forward`, with a seq of the device's and a
//! tag that says whose they are), answers come back through the device's task, which puts the client's seq back on
//! them. Both hold the device's state while they decide and send, so the device gets commands in the order they were
//! decided. Streams are opened on the device by the first client and closed by the last, their data
//! goes to the clients that opened them; observed properties are merged from every client's settings; introspection
//! is fetched once and answered from the cache after.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use tracing::{debug, info, warn};
use wire_weaver::shrink_wrap::UNib32;
use wire_weaver_client::internal::{Command, ForwardSink, Forwarded};
use wire_weaver_client::ww_client_server::observe::{
    ObserveEntry, ObserveMode, ObserveRequestOwned,
};
use wire_weaver_client::ww_client_server::{RequestKindOwned, StreamSideband};
use wire_weaver_client::ww_self::ApiBundleOwned;
use wire_weaver_client::{ClientConfig, ClientKind, ClientMeta, Commander, WwClient};
use ww_device::ww_link::Kind;
use ww_local_socket::Status;

use crate::Hub;
use crate::claims::{self, Target};
use crate::envelope::{self, Op, Place, Unsolicited};
use crate::tunnel::{DeviceLinkInfo, TunnelQueue};

/// A forwarded request whose answer doesn't come for this long is given up (its seq freed on the device): longer
/// than any client waits, as the hub doesn't know a call's own timeout.
const FORWARD_TIMEOUT: Duration = Duration::from_secs(600);

/// How long a lend waits for the device's event loop to let the device go.
const LEND_CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

/// The hub itself as the requester of an introspection fetch (for claims): no tunnel has this id.
const HUB_ITSELF: u32 = u32::MAX;

/// What a tunnel needs of an open device.
#[derive(Clone)]
pub(crate) struct DeviceHandle {
    pub(crate) tx: mpsc::UnboundedSender<ToDevice>,
    pub(crate) info: Arc<DeviceLinkInfo>,
    pub(crate) device: Arc<Device>,
}

pub(crate) enum ToDevice {
    Join {
        id: u32,
        queue: Arc<TunnelQueue>,
    },
    Leave {
        id: u32,
    },
    /// `Registry::take`: keep it open without clients
    Keep,
    /// The device's API from its introspection data (fetched once, from the cache after), for claims (SRV-34);
    /// `None` when the device has none or it can't be read
    Bundle {
        reply: oneshot::Sender<Option<Arc<ApiBundleOwned>>>,
    },
    /// The hub exits: close the device now
    Close,
    /// Nothing: sent to have the device opened (a lent one returned)
    Open,
    /// Lent to a program outside the hub (SRV-32): close the tunnels saying so, close the device, then answer how
    /// the hub held it
    Lend {
        done: oneshot::Sender<claims::Lend>,
    },
    /// Whether the device is steered changed (SRV-31): its linger is looked at again
    Recheck,
}

/// One slot per device key: whoever opens or closes the device holds it, so a tunnel joins either the running task or
/// a new one, never one that is about to end.
pub(crate) type Slot = Arc<tokio::sync::Mutex<Option<DeviceHandle>>>;

/// The event loop's client handle, nothing generated: the hub never links a device's API.
struct Raw(Commander);

impl WwClient for Raw {
    fn default_config() -> ClientConfig {
        ClientConfig::new()
    }

    fn from_cmd(cmd: Commander) -> Self {
        Raw(cmd)
    }
}

/// Whether the hub knows how to reach a device by `key`'s scheme.
pub(crate) fn has_transport(key: &str) -> bool {
    config_for(key).is_some()
}

/// How the hub reaches the device behind a key: `usb:<vid>:<pid>:<serial>[:<iface>]`, `rtt:<probe serial>:<chip>`
/// (`holders::key_for`), `inproc:<path>`, a device served in the hub's own process (`wire_weaver_client::in_process`:
/// tests, simulators hosted with the hub), or `ipc:<socket path>`, a device another process serves on a local socket
/// (`wire_weaver_client::local_socket::device`: a simulator, host-emulated firmware), which outlives the hub.
fn config_for(key: &str) -> Option<ClientConfig> {
    let (scheme, rest) = key.split_once(':')?;
    let config = match scheme {
        "inproc" => ClientConfig::new().in_process_path(rest.to_string()),
        "ipc" => ClientConfig::new().ipc_path(rest.to_string()),
        "usb" => {
            let mut parts = rest.splitn(4, ':');
            let vid = u16::from_str_radix(parts.next()?, 16).ok()?;
            let pid = u16::from_str_radix(parts.next()?, 16).ok()?;
            let serial = parts.next()?;
            let config = ClientConfig::new().usb_vid_pid(vid, pid);
            if serial.is_empty() {
                config
            } else {
                config.serial_eq(serial.to_string())
            }
        }
        "rtt" => {
            // `rtt:<probe serial>:<chip>`; filters after `rtt` pick the probe. Needs `wire_weaver_client`'s `rtt`
            // feature, which `ww` has: without it the open fails with a message that says so.
            let (probe, chip) = rest.split_once(':')?;
            ClientConfig::new()
                .rtt(chip.to_string(), None)
                .serial_eq(probe.to_string())
        }
        _ => return None,
    };
    // what the hub forwards was checked by its clients, and it never calls the device's API itself; it opens its
    // devices itself, never through a hub (CLT-39 would route it to itself)
    Some(
        config
            .traits_only()
            .hub(wire_weaver_client::hub::HubMode::Direct)
            .holder_mode(wire_weaver_client::holders::Mode::Hub)
            .client_meta(ClientMeta::new().kind(ClientKind::Hub).program("ww hub")),
    )
}

/// The device behind `key` with the tunnel `id` joined: the task the hub runs for it, or a new one if the hub doesn't
/// hold it yet. The preamble's answer when it can't.
pub(crate) async fn join(
    hub: &Arc<Hub>,
    key: &str,
    id: u32,
    queue: &Arc<TunnelQueue>,
) -> Result<DeviceHandle, Status> {
    let slot = hub.device_slot(key);
    let mut guard = slot.lock().await;
    if let Some(handle) = guard.as_ref()
        && handle
            .tx
            .send(ToDevice::Join {
                id,
                queue: queue.clone(),
            })
            .is_ok()
    {
        return Ok(handle.clone());
    }
    if hub.lent_to(key).is_some() {
        return Err(Status::Lent);
    }
    let handle = open(hub, key, slot.clone()).await?;
    let _ = handle.tx.send(ToDevice::Join {
        id,
        queue: queue.clone(),
    });
    *guard = Some(handle.clone());
    Ok(handle)
}

/// Opens the device and starts its task, which ends after [crate::HubConfig::linger] without tunnels.
pub(crate) async fn open(hub: &Arc<Hub>, key: &str, slot: Slot) -> Result<DeviceHandle, Status> {
    let Some(config) = config_for(key) else {
        debug!("ww hub: no transport for device key {key:?}");
        return Err(Status::UnknownDevice);
    };
    let cmd = match wire_weaver_client::PreparedConnection::<Raw>::new(config)
        .connect()
        .await
    {
        Ok(Raw(cmd)) => cmd,
        Err(e) => {
            let e = format!("{e:#}");
            warn!("ww hub: opening {key} failed: {e}");
            return Err(if e.contains("already open by another") {
                Status::HeldElsewhere
            } else {
                Status::UnknownDevice
            });
        }
    };
    let (sink_tx, sink) = mpsc::unbounded_channel();
    let routes = Arc::new(Routes::default());
    let router = Router {
        routes: routes.clone(),
        task: sink_tx,
    };
    if cmd
        .send(Command::ForwardSink {
            sink: Box::new(router),
        })
        .await
        .is_err()
    {
        return Err(Status::UnknownDevice);
    }
    info!(
        "ww hub: opened {key}: {:?}",
        cmd.device_api_info().user_api_version
    );
    let info = Arc::new(DeviceLinkInfo::new(cmd.device_api_info().clone()));
    let (tx, rx) = mpsc::unbounded_channel();
    let device = Arc::new(Device {
        state: tokio::sync::Mutex::new(State {
            tunnels: BTreeMap::new(),
            next_tag: 1,
            pending: HashMap::new(),
            streams: HashMap::new(),
            observed: HashMap::new(),
            introspect: Introspect::None,
            bundle: None,
            bundle_waiters: vec![],
            key: key.to_string(),
            loopback_owner: None,
            keep: false,
            stamp: 0,
            routes,
            out: vec![],
        }),
        cmd,
        key: key.to_string(),
        hub: hub.clone(),
    });
    let task = Task {
        key: key.to_string(),
        hub: hub.clone(),
        device: device.clone(),
    };
    hub.device_opened(key);
    crate::sync::start(hub, key, device.clone(), &tx);
    tokio::spawn(task.run(rx, sink, slot, tx.clone()));
    Ok(DeviceHandle { tx, info, device })
}

/// Tags of plain requests carry whose they are: this bit, the tunnel (31 bits) and the client's seq. Their answers go
/// from the event loop's receive task straight into the client's queue ([Router]), no task in between.
const DIRECT: u64 = 1 << 63;

fn direct_tag(tunnel: u32, seq: u32) -> u64 {
    DIRECT | (u64::from(tunnel & 0x7FFF_FFFF) << 32) | u64::from(seq)
}

/// What [Router] needs to put answers into the clients' queues on its own: a copy of the device's tunnels.
#[derive(Default)]
struct Routes {
    tunnels: std::sync::Mutex<BTreeMap<u32, Arc<TunnelQueue>>>,
    /// The client whose loopback messages went out last, 0 for none
    loopback_owner: std::sync::atomic::AtomicU32,
}

impl Routes {
    fn queue(&self, tunnel: u32) -> Option<Arc<TunnelQueue>> {
        self.tunnels.lock().unwrap().get(&tunnel).cloned()
    }
}

/// The device's event loop forwards here, on its receive task: answers to plain requests and loopback echoes go
/// into the client's queue right away, the rest (answers the hub looks at, stream data, property updates) to the
/// device's task. Dropped with the event loop, which ends the task's channel: the device is gone.
struct Router {
    routes: Arc<Routes>,
    task: mpsc::UnboundedSender<Forwarded>,
}

impl ForwardSink for Router {
    fn forward(&mut self, forwarded: Forwarded) {
        match forwarded {
            Forwarded::Reply { tag, event, .. } if tag & DIRECT != 0 => {
                let (tunnel, seq) = (((tag >> 32) as u32) & 0x7FFF_FFFF, tag as u32);
                if let Some(queue) = self.routes.queue(tunnel)
                    && let Some((_, rest)) = envelope::split_seq(&event)
                {
                    queue.push(Kind::Data0, envelope::with_seq(seq, rest));
                }
            }
            // the client times out on its own
            Forwarded::Failed { tag, .. } if tag & DIRECT != 0 => {}
            Forwarded::Loopback(payload) => {
                let owner = self.routes.loopback_owner.load(Ordering::Relaxed);
                if let Some(queue) = self.routes.queue(owner) {
                    queue.push(Kind::Loopback, payload);
                }
            }
            other => _ = self.task.send(other),
        }
    }
}

/// Whose request a forwarded one is, and what the hub does with its answer besides passing it on.
struct Pending {
    tunnel: u32,
    seq: u32,
    purpose: Purpose,
}

enum Purpose {
    Plain,
    /// Opens the stream at the path: the first client's, others are answered with its answer
    StreamOpen(Vec<u32>),
    Introspect,
}

/// A stream at one path, opened on the device.
#[derive(Default)]
struct Stream {
    /// Clients that opened it, in order: the first one's acknowledgements and resend requests go to the device
    subs: Vec<u32>,
    /// The device's answer to the open (after its seq), for the clients that open it later
    opened: Option<Vec<u8>>,
    /// An open is on its way to the device
    opening: bool,
    /// Opens that wait for its answer: client and request
    waiting: Vec<(u32, Vec<u8>)>,
}

/// Observed properties of one trait level: what each client asked for.
#[derive(Default)]
struct Level {
    /// Per client: settings by resource index, with when they were set (the later setting of a property wins)
    entries: BTreeMap<u32, BTreeMap<u32, (u64, ObserveEntry)>>,
    budgets: BTreeMap<u32, Option<UNib32>>,
}

impl Level {
    fn apply(&mut self, tunnel: u32, cfg: &ObserveRequestOwned, stamp: u64) {
        let own = self.entries.entry(tunnel).or_default();
        if !cfg.patch {
            own.clear();
        }
        for e in &cfg.entries {
            if matches!(e.mode, ObserveMode::Off) {
                own.remove(&e.index.0);
            } else {
                own.insert(e.index.0, (stamp, *e));
            }
        }
        if own.is_empty() {
            self.entries.remove(&tunnel);
            self.budgets.remove(&tunnel);
        } else {
            self.budgets.insert(tunnel, cfg.budget);
        }
    }

    fn remove(&mut self, tunnel: u32) -> bool {
        self.budgets.remove(&tunnel);
        self.entries.remove(&tunnel).is_some()
    }

    /// Every client's properties, each with its latest setting, the largest budget asked for.
    fn merged(&self) -> ObserveRequestOwned {
        let mut merged: BTreeMap<u32, (u64, ObserveEntry)> = BTreeMap::new();
        for own in self.entries.values() {
            for (index, (stamp, e)) in own {
                match merged.get(index) {
                    Some((s, _)) if s > stamp => {}
                    _ => {
                        merged.insert(*index, (*stamp, *e));
                    }
                }
            }
        }
        ObserveRequestOwned {
            entries: merged.into_values().map(|(_, e)| e).collect(),
            patch: false,
            budget: self
                .budgets
                .values()
                .flatten()
                .map(|b| b.0)
                .max()
                .map(UNib32),
        }
    }

    fn tunnels(&self) -> impl Iterator<Item = u32> + '_ {
        self.entries.keys().copied()
    }
}

enum Introspect {
    None,
    /// Asked the device for a client (`requester`, with its seq): the answers so far (after their seq), the request,
    /// and clients that wait. A client asks with seq 0 and takes the stream data at the root path, which carries the
    /// request's seq.
    Fetching {
        events: Vec<Vec<u8>>,
        request: Vec<u8>,
        requester: (u32, u32),
        waiting: Vec<(u32, u32)>,
    },
    /// Every event of the answer, after its seq
    Cached(Vec<Vec<u8>>),
}

struct State {
    tunnels: BTreeMap<u32, Arc<TunnelQueue>>,
    next_tag: u64,
    pending: HashMap<u64, Pending>,
    streams: HashMap<Vec<u32>, Stream>,
    observed: HashMap<Vec<u32>, Level>,
    introspect: Introspect,
    /// The device's API decoded from the cached introspection data, once someone needed it (SRV-34)
    bundle: Option<Arc<ApiBundleOwned>>,
    /// Asked for the API while the introspection data is on its way
    bundle_waiters: Vec<oneshot::Sender<Option<Arc<ApiBundleOwned>>>>,
    /// The device's key, for messages
    key: String,
    /// The client whose loopback messages went out last gets the echoes
    loopback_owner: Option<u32>,
    /// `Registry::take`: no linger
    keep: bool,
    /// Orders observe settings
    stamp: u64,
    /// The tunnels again, for [Router]
    routes: Arc<Routes>,
    /// Commands for the event loop, decided with the state held and sent before it is let go: the device gets them
    /// in the order they were decided
    out: Vec<Command>,
}

/// An open device: its state, shared by its tunnels (requests) and its task (answers, joins and leaves).
pub(crate) struct Device {
    state: tokio::sync::Mutex<State>,
    pub(crate) cmd: Commander,
    key: String,
    hub: Arc<Hub>,
}

impl Device {
    /// A client's request: forwarded with a seq of the device's, or answered by the hub. Straight from the client's
    /// tunnel to the event loop, no task in between. `last` of what the client wrote at once: the device gets the
    /// batch now, with the flush right behind the request, so the event loop takes both in one go.
    pub(crate) async fn request(&self, id: u32, bytes: Vec<u8>, last: bool) -> Result<(), ()> {
        let mut state = self.state.lock().await;
        if let Some((seq, reply)) = self.refused(&mut state, id, &bytes) {
            if seq != 0
                && let Some(queue) = state.tunnels.get(&id)
            {
                queue.push(Kind::Data0, reply);
            }
            return Ok(());
        }
        state.request(id, bytes)?;
        self.send_out(&mut state, false).await?;
        self.flush_if(last).await
    }

    /// The answer to a request another program's claim refuses (SRV-34) with the request's seq, `None` when it may
    /// go to the device.
    fn refused(&self, state: &mut State, id: u32, bytes: &[u8]) -> Option<(u32, Vec<u8>)> {
        let owner = {
            let hub = self.hub.state.lock().unwrap();
            if !hub.claims.any_on(&self.key) {
                return None;
            }
            crate::owner_of(&hub, id)
        };
        let (seq, _) = envelope::split_seq(bytes)?;
        let (access, target, path) = match envelope::access(bytes) {
            Ok(None) => return None,
            Ok(Some((access, place, multi))) => {
                let path = match place {
                    Place::Absolute(path) => Some(path),
                    Place::Trait(trait_id, from_trait) => state
                        .bundle()
                        .and_then(|b| claims::trait_path(&b, &trait_id))
                        .map(|mut p| {
                            p.extend(from_trait);
                            p
                        }),
                };
                let target = match (&path, multi) {
                    (Some(p), false) => Target::At(p.clone()),
                    (Some(p), true) => Target::Under(p.clone()),
                    (None, _) => Target::Anywhere,
                };
                (access, target, path.filter(|_| !multi))
            }
            // a newer envelope than this build: it can't be placed, so not past a claim
            Err(()) => (claims::Access::Call, Target::Anywhere, None),
        };
        let by = {
            let mut hub = self.hub.state.lock().unwrap();
            let expired = hub.claims.expire(Instant::now());
            let by = hub
                .claims
                .refusing(&self.key, &owner, &target, access)
                .map(|c| c.by());
            drop(hub);
            self.hub.claims_released(&expired);
            by?
        };
        debug!(
            "ww hub: {} refused for client {id}: in use by {by}",
            self.key
        );
        let bundle = state.bundle();
        Some((
            seq,
            claims::in_use_reply(bundle.as_deref(), path.as_deref(), access, &by, seq),
        ))
    }

    pub(crate) async fn loopback(
        &self,
        id: u32,
        repeat: u32,
        seq: u32,
        data: Vec<u8>,
        last: bool,
    ) -> Result<(), ()> {
        let mut state = self.state.lock().await;
        state.loopback_owner = Some(id);
        state.routes.loopback_owner.store(id, Ordering::Relaxed);
        state.send(Command::ForwardLoopback { repeat, seq, data })?;
        self.send_out(&mut state, false).await?;
        self.flush_if(last).await
    }

    /// The client flushed: so does the hub.
    async fn flush_if(&self, last: bool) -> Result<(), ()> {
        match last {
            true => self.cmd.send(Command::ForwardFlush).await.map_err(|_| ()),
            false => Ok(()),
        }
    }

    /// Hands what `state` queued to the event loop. Forwarded requests wait there for a flush: a client's for the
    /// end of its batch (`last`), the hub's own (`flush`) go out at once.
    async fn send_out(&self, state: &mut State, flush: bool) -> Result<(), ()> {
        let sent = !state.out.is_empty();
        for cmd in state.out.drain(..) {
            self.cmd.send(cmd).await.map_err(|_| ())?;
        }
        if flush && sent {
            self.cmd.send(Command::ForwardFlush).await.map_err(|_| ())?;
        }
        Ok(())
    }
}

/// The task of an open device: answers and events from the event loop, tunnels joining and leaving, linger.
struct Task {
    key: String,
    hub: Arc<Hub>,
    device: Arc<Device>,
}

impl Task {
    async fn run(
        self,
        mut rx: mpsc::UnboundedReceiver<ToDevice>,
        mut sink: mpsc::UnboundedReceiver<Forwarded>,
        slot: Slot,
        tx: mpsc::UnboundedSender<ToDevice>,
    ) {
        drop(tx);
        let mut idle_since = Some(Instant::now());
        // the slot no longer names this task
        let mut released = false;
        let mut keep = false;
        // lent out (SRV-32): who waits for the device to be closed
        let mut lend = None;
        loop {
            // a steered device lingers longer (SRV-31), maybe forever
            let linger = self.hub.linger_for(&self.key);
            let linger_at = idle_since
                .filter(|_| !keep)
                .and_then(|t| linger.map(|l| t + l));
            let timer = async {
                match linger_at {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                msg = rx.recv() => {
                    let Some(msg) = msg else { break };
                    if let ToDevice::Lend { done } = msg {
                        // the hub took the handle out of the slot already
                        released = true;
                        rx.close();
                        lend = Some(done);
                        break;
                    }
                    let mut state = self.device.state.lock().await;
                    let r = state.on_message(msg);
                    if self.device.send_out(&mut state, true).await.is_err() || r.is_err() {
                        break;
                    }
                    let tunnels = state.tunnels.len();
                    keep = state.keep;
                    drop(state);
                    self.hub.tunnels_changed(&self.key, tunnels);
                    idle_since = match tunnels == 0 {
                        true => idle_since.or(Some(Instant::now())),
                        false => None,
                    };
                }
                fwd = sink.recv() => match fwd {
                    Some(fwd) => {
                        let mut state = self.device.state.lock().await;
                        state.on_forwarded(fwd);
                        if self.device.send_out(&mut state, true).await.is_err() {
                            break;
                        }
                    }
                    None => {
                        info!("ww hub: {} is gone", self.key);
                        break;
                    }
                },
                _ = timer => {
                    // nobody may join while it closes: a tunnel waiting for the slot opens it anew after
                    let mut guard = slot.lock().await;
                    if !rx.is_empty() {
                        continue;
                    }
                    *guard = None;
                    rx.close();
                    released = true;
                    drop(guard);
                    debug!("ww hub: closing {} after {linger:?} without clients", self.key);
                    let _ = self
                        .device
                        .cmd
                        .send(Command::disconnect_and_exit(wire_weaver::DisconnectReason::RequestByUser).0)
                        .await;
                    break;
                }
            }
        }
        // the device is gone, or closed: tunnels that joined meanwhile learn it too
        if !released {
            let mut guard = slot.lock().await;
            *guard = None;
            rx.close();
        }
        while let Ok(msg) = rx.try_recv() {
            match msg {
                ToDevice::Join { queue, .. } if lend.is_some() => queue.close_lent(),
                ToDevice::Join { queue, .. } => queue.close(),
                // a lend that came after another close: the device is closed
                ToDevice::Lend { done } => _ = done.send(claims::Lend::default()),
                _ => {}
            }
        }
        for queue in self.device.state.lock().await.tunnels.values() {
            match lend.is_some() {
                true => queue.close_lent(),
                false => queue.close(),
            }
        }
        if let Some(done) = lend {
            // the other program opens the device right after: wait until the event loop let it go
            let (cmd, closed) =
                Command::disconnect_and_exit(wire_weaver::DisconnectReason::RequestByUser);
            if self.device.cmd.send(cmd).await.is_ok()
                && tokio::time::timeout(LEND_CLOSE_TIMEOUT, closed)
                    .await
                    .is_err()
            {
                warn!(
                    "ww hub: {} not closed within {LEND_CLOSE_TIMEOUT:?} for its lend",
                    self.key
                );
            }
            self.hub.device_closed(&self.key);
            let _ = done.send(claims::Lend { reopen: true, keep });
            return;
        }
        self.hub.device_closed(&self.key);
    }
}

impl State {
    fn on_message(&mut self, msg: ToDevice) -> Result<(), ()> {
        match msg {
            ToDevice::Join { id, queue } => {
                self.tunnels.insert(id, queue.clone());
                self.routes.tunnels.lock().unwrap().insert(id, queue);
            }
            ToDevice::Leave { id } => self.leave(id)?,
            ToDevice::Keep => self.keep = true,
            ToDevice::Open => {}
            // taken by the task itself
            ToDevice::Lend { done } => _ = done.send(claims::Lend::default()),
            ToDevice::Bundle { reply } => match &self.introspect {
                Introspect::Cached(_) => {
                    let _ = reply.send(self.bundle());
                }
                Introspect::Fetching { .. } => self.bundle_waiters.push(reply),
                Introspect::None => {
                    self.bundle_waiters.push(reply);
                    let request = envelope::request(&[], RequestKindOwned::Introspect)
                        .and_then(|r| envelope::split_seq(&r).map(|(_, rest)| rest.to_vec()))
                        .unwrap_or_default();
                    // asked with seq 0: the data comes as stream data at the root path, to nobody's tunnel
                    self.introspect = Introspect::Fetching {
                        events: vec![],
                        request: request.clone(),
                        requester: (HUB_ITSELF, 0),
                        waiting: vec![],
                    };
                    self.forward(HUB_ITSELF, 0, &request, Purpose::Introspect)?;
                }
            },
            ToDevice::Recheck => {}
            ToDevice::Close => {
                let _ = self.send(
                    Command::disconnect_and_exit(wire_weaver::DisconnectReason::RequestByUser).0,
                );
                return Err(());
            }
        }
        Ok(())
    }

    /// Queues a command for the event loop, sent by whoever holds the state.
    fn send(&mut self, cmd: Command) -> Result<(), ()> {
        self.out.push(cmd);
        Ok(())
    }

    /// A client's request: forwarded with a seq of the device's, or answered by the hub.
    fn request(&mut self, id: u32, bytes: Vec<u8>) -> Result<(), ()> {
        let Some((seq, rest)) = envelope::split_seq(&bytes) else {
            return Ok(());
        };
        match envelope::classify(&bytes) {
            Op::StreamOpen(path) => self.stream_open(id, seq, bytes, path),
            Op::StreamClose(path) => {
                let forward = match self.streams.get_mut(&path) {
                    // not opened through the hub's accounting (e.g. by a trait path): as it is
                    None => true,
                    Some(stream) => {
                        stream.subs.retain(|t| *t != id);
                        let last =
                            stream.subs.is_empty() && !stream.opening && stream.waiting.is_empty();
                        if last {
                            self.streams.remove(&path);
                        }
                        last
                    }
                };
                if forward {
                    self.forward(id, seq, rest, Purpose::Plain)
                } else {
                    self.answer_written(id, seq);
                    Ok(())
                }
            }
            Op::StreamSideband(path) => match self.streams.get(&path) {
                // acknowledgements, resends and credit of a shared stream are the first client's
                Some(stream) if stream.subs.first().is_some_and(|t| *t != id) => {
                    self.answer_written(id, seq);
                    Ok(())
                }
                _ => self.forward(id, seq, rest, Purpose::Plain),
            },
            Op::Observe(path, cfg) => {
                self.stamp += 1;
                let level = self.observed.entry(path.clone()).or_default();
                level.apply(id, &cfg, self.stamp);
                let merged = level.merged();
                if level.entries.is_empty() {
                    self.observed.remove(&path);
                }
                match envelope::request(&path, RequestKindOwned::Observe { cfg: merged }) {
                    // the device answers what it grants for all of them
                    Some(req) => {
                        let rest = envelope::split_seq(&req).map(|(_, r)| r.to_vec());
                        self.forward(id, seq, &rest.unwrap_or_default(), Purpose::Plain)
                    }
                    None => self.forward(id, seq, rest, Purpose::Plain),
                }
            }
            Op::Introspect => match &mut self.introspect {
                Introspect::Cached(events) => {
                    if let Some(queue) = self.tunnels.get(&id) {
                        for e in events {
                            queue.push(Kind::Data0, envelope::with_seq(seq, e));
                        }
                    }
                    Ok(())
                }
                Introspect::Fetching { waiting, .. } => {
                    waiting.push((id, seq));
                    Ok(())
                }
                Introspect::None => {
                    self.introspect = Introspect::Fetching {
                        events: vec![],
                        request: rest.to_vec(),
                        requester: (id, seq),
                        waiting: vec![],
                    };
                    self.forward(id, seq, rest, Purpose::Introspect)
                }
            },
            Op::Other => self.forward(id, seq, rest, Purpose::Plain),
        }
    }

    fn stream_open(&mut self, id: u32, seq: u32, bytes: Vec<u8>, path: Vec<u32>) -> Result<(), ()> {
        let stream = self.streams.entry(path.clone()).or_default();
        let alone = stream.subs.iter().all(|t| *t == id);
        if alone || (stream.opened.is_none() && !stream.opening) {
            // the first, or the only one (a reopen, a resume): the device's own
            if !stream.subs.contains(&id) {
                stream.subs.push(id);
            }
            stream.opening = true;
            let rest = envelope::split_seq(&bytes).map(|(_, r)| r.to_vec());
            return self.forward(
                id,
                seq,
                &rest.unwrap_or_default(),
                Purpose::StreamOpen(path),
            );
        }
        if stream.opening {
            stream.waiting.push((id, bytes));
            return Ok(());
        }
        if !stream.subs.contains(&id) {
            stream.subs.push(id);
        }
        if let (Some(opened), Some(queue)) = (&stream.opened, self.tunnels.get(&id))
            && seq != 0
        {
            queue.push(Kind::Data0, envelope::with_seq(seq, opened));
        }
        Ok(())
    }

    /// The device's API, decoded from the cached introspection data on first use.
    fn bundle(&mut self) -> Option<Arc<ApiBundleOwned>> {
        if self.bundle.is_none()
            && let Introspect::Cached(events) = &self.introspect
        {
            let bytes = envelope::introspection_bytes(events);
            match wire_weaver_client::decode_introspection(&bytes) {
                Ok(bundle) => self.bundle = bundle.map(Arc::new),
                Err(e) => warn!("ww hub: introspection data of {}: {e:#}", self.key),
            }
        }
        self.bundle.clone()
    }

    fn answer_written(&self, id: u32, seq: u32) {
        if seq != 0
            && let Some(queue) = self.tunnels.get(&id)
        {
            queue.push(Kind::Data0, envelope::written(seq));
        }
    }

    /// Sends `rest` (a request after its seq) to the device; its answers come back with a tag that says whose.
    fn forward(&mut self, id: u32, seq: u32, rest: &[u8], purpose: Purpose) -> Result<(), ()> {
        let tag = (seq != 0).then(|| {
            if let Purpose::Plain = purpose {
                return direct_tag(id, seq);
            }
            let tag = self.next_tag;
            self.next_tag += 1;
            self.pending.insert(
                tag,
                Pending {
                    tunnel: id,
                    seq,
                    purpose,
                },
            );
            tag
        });
        self.send(Command::Forward {
            bytes: envelope::with_placeholder(rest),
            tag,
            timeout: FORWARD_TIMEOUT,
        })
    }

    fn leave(&mut self, id: u32) -> Result<(), ()> {
        self.tunnels.remove(&id);
        self.routes.tunnels.lock().unwrap().remove(&id);
        if self.loopback_owner == Some(id) {
            self.loopback_owner = None;
            self.routes.loopback_owner.store(0, Ordering::Relaxed);
        }
        let mut close = vec![];
        self.streams.retain(|path, stream| {
            stream.subs.retain(|t| *t != id);
            stream.waiting.retain(|(t, _)| *t != id);
            let last = stream.subs.is_empty() && !stream.opening && stream.waiting.is_empty();
            if last {
                close.push(path.clone());
            }
            !last
        });
        // nobody else reads them: closed on the device as the client would have
        for path in close {
            if let Some(req) = envelope::request(
                &path,
                RequestKindOwned::StreamSideband {
                    sideband: StreamSideband::Close,
                },
            ) {
                self.send(Command::Forward {
                    bytes: req,
                    tag: None,
                    timeout: FORWARD_TIMEOUT,
                })?;
            }
        }
        let mut changed = vec![];
        self.observed.retain(|path, level| {
            if level.remove(id) {
                changed.push((path.clone(), level.merged()));
            }
            !level.entries.is_empty()
        });
        for (path, merged) in changed {
            if let Some(req) = envelope::request(&path, RequestKindOwned::Observe { cfg: merged }) {
                self.send(Command::Forward {
                    bytes: req,
                    tag: None,
                    timeout: FORWARD_TIMEOUT,
                })?;
            }
        }
        if let Introspect::Fetching {
            waiting, requester, ..
        } = &mut self.introspect
        {
            waiting.retain(|(t, _)| *t != id);
            if requester.0 == id {
                // the next one waiting asks again
                self.introspected(None);
            }
        }
        Ok(())
    }

    fn on_forwarded(&mut self, fwd: Forwarded) {
        match fwd {
            Forwarded::Reply { tag, event, last } => self.reply(tag, event, last),
            Forwarded::Event(event) => self.fan_out(event),
            Forwarded::Loopback(payload) => {
                if let Some(queue) = self.loopback_owner.and_then(|t| self.tunnels.get(&t)) {
                    queue.push(Kind::Loopback, payload);
                }
            }
            Forwarded::Failed { tag, error } => {
                let Some(pending) = self.pending.remove(&tag) else {
                    return;
                };
                debug!(
                    "ww hub: request of tunnel {} failed: {error}",
                    pending.tunnel
                );
                // the client times out on its own; what waited on it is tried again
                match pending.purpose {
                    Purpose::Plain => {}
                    Purpose::StreamOpen(path) => self.opened(pending.tunnel, path, None),
                    Purpose::Introspect => self.introspected(None),
                }
            }
        }
    }

    fn reply(&mut self, tag: u64, event: Vec<u8>, last: bool) {
        let Some(pending) = self.pending.get(&tag) else {
            return;
        };
        let (tunnel, seq) = (pending.tunnel, pending.seq);
        let introspect = matches!(pending.purpose, Purpose::Introspect);
        let Some((_, rest)) = envelope::split_seq(&event) else {
            return;
        };
        if let Some(queue) = self.tunnels.get(&tunnel) {
            queue.push(Kind::Data0, envelope::with_seq(seq, rest));
        }
        if introspect && let Introspect::Fetching { events, .. } = &mut self.introspect {
            events.push(rest.to_vec());
        }
        if !last {
            return;
        }
        let Some(pending) = self.pending.remove(&tag) else {
            return;
        };
        match pending.purpose {
            Purpose::Plain => {}
            Purpose::StreamOpen(path) => {
                let opened = envelope::is_ok(&event).then(|| rest.to_vec());
                self.opened(tunnel, path, opened);
            }
            Purpose::Introspect => self.introspected(Some(())),
        }
    }

    /// The device answered a stream open of `tunnel` (`None`: with an error, or not at all).
    fn opened(&mut self, tunnel: u32, path: Vec<u32>, opened: Option<Vec<u8>>) {
        let Some(stream) = self.streams.get_mut(&path) else {
            return;
        };
        stream.opening = false;
        let waiting = std::mem::take(&mut stream.waiting);
        match opened {
            Some(opened) => stream.opened = Some(opened),
            None => {
                stream.subs.retain(|t| *t != tunnel);
                if stream.subs.is_empty() {
                    stream.opened = None;
                }
            }
        }
        if stream.subs.is_empty() && waiting.is_empty() {
            self.streams.remove(&path);
        }
        // answered from the cache, or the next one opens it
        for (t, bytes) in waiting {
            if self.request(t, bytes).is_err() {
                return;
            }
        }
    }

    /// The introspection fetch ended: cached for everyone after, or tried again for the next one waiting.
    fn introspected(&mut self, done: Option<()>) {
        let Introspect::Fetching {
            events,
            request,
            waiting,
            ..
        } = std::mem::replace(&mut self.introspect, Introspect::None)
        else {
            return;
        };
        match done {
            Some(()) => {
                for (t, seq) in &waiting {
                    if let Some(queue) = self.tunnels.get(t) {
                        for e in &events {
                            queue.push(Kind::Data0, envelope::with_seq(*seq, e));
                        }
                    }
                }
                self.introspect = Introspect::Cached(events);
                if !self.bundle_waiters.is_empty() {
                    let bundle = self.bundle();
                    for reply in self.bundle_waiters.drain(..) {
                        let _ = reply.send(bundle.clone());
                    }
                }
            }
            None => {
                let mut waiting = waiting.into_iter();
                if let Some((t, seq)) = waiting.next() {
                    self.introspect = Introspect::Fetching {
                        events: vec![],
                        request: request.clone(),
                        requester: (t, seq),
                        waiting: waiting.collect(),
                    };
                    let _ = self.forward(t, seq, &request, Purpose::Introspect);
                } else {
                    // nobody left to ask for it: claims go without the API
                    for reply in self.bundle_waiters.drain(..) {
                        let _ = reply.send(None);
                    }
                }
            }
        }
    }

    /// An event nobody asked for (seq 0): to the clients of its stream or observed level, to all when none is known.
    fn fan_out(&mut self, event: Vec<u8>) {
        let class = envelope::classify_event(&event);
        // introspection data asked for with seq 0, at the root path
        if let Introspect::Fetching {
            events, requester, ..
        } = &mut self.introspect
            && let Unsolicited::Data(path) | Unsolicited::Sideband { path, .. } = &class
            && path.is_empty()
        {
            if let Some((_, rest)) = envelope::split_seq(&event) {
                events.push(rest.to_vec());
            }
            if let Some(queue) = self.tunnels.get(&requester.0) {
                queue.push(Kind::Data0, event);
            }
            if matches!(class, Unsolicited::Sideband { .. }) {
                self.introspected(Some(()));
            }
            return;
        }
        let (targets, data): (Option<Vec<u32>>, bool) = match class {
            Unsolicited::Data(path) => (self.streams.get(&path).map(|s| s.subs.clone()), true),
            Unsolicited::Sideband { path, close } => {
                let subs = match close {
                    // the device ended it: the next open goes to the device again
                    true => self.streams.remove(&path).map(|s| s.subs),
                    false => self.streams.get(&path).map(|s| s.subs.clone()),
                };
                (subs, false)
            }
            Unsolicited::PropertyUpdate(path) => (
                self.observed.get(&path).map(|l| l.tunnels().collect()),
                true,
            ),
            Unsolicited::Other => (None, false),
        };
        let send = |queue: &TunnelQueue, bytes: Vec<u8>| match data {
            true => queue.push_data(bytes),
            false => queue.push(Kind::Data0, bytes),
        };
        match targets {
            Some(targets) => {
                let mut targets = targets
                    .iter()
                    .filter_map(|t| self.tunnels.get(t))
                    .peekable();
                while let Some(queue) = targets.next() {
                    match targets.peek() {
                        Some(_) => send(queue, event.clone()),
                        None => {
                            send(queue, event);
                            break;
                        }
                    }
                }
            }
            // e.g. property changes sent by hand as stream data: whoever listens takes them
            None => {
                for queue in self.tunnels.values() {
                    send(queue, event.clone());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(index: u32, mode: ObserveMode) -> ObserveEntry {
        ObserveEntry {
            index: UNib32(index),
            mode,
        }
    }

    fn cfg(entries: Vec<ObserveEntry>, patch: bool, budget: Option<u32>) -> ObserveRequestOwned {
        ObserveRequestOwned {
            entries,
            patch,
            budget: budget.map(UNib32),
        }
    }

    /// Observe settings of several clients: every property anyone observes, the later setting of one wins, the
    /// largest budget; a client that leaves or stops takes only its own away.
    #[test]
    fn observe_merged() {
        let periodic = |us| ObserveMode::Periodic {
            every_us: UNib32(us),
        };
        let on_change = ObserveMode::OnChange { min_us: UNib32(0) };
        let mut level = Level::default();
        level.apply(
            1,
            &cfg(
                vec![entry(0, periodic(1000)), entry(2, on_change)],
                false,
                Some(100),
            ),
            1,
        );
        level.apply(
            2,
            &cfg(
                vec![entry(2, periodic(50)), entry(3, on_change)],
                false,
                None,
            ),
            2,
        );
        let m = level.merged();
        assert_eq!(
            m.entries,
            [
                entry(0, periodic(1000)),
                entry(2, periodic(50)),
                entry(3, on_change)
            ]
        );
        assert!(!m.patch);
        assert_eq!(m.budget, Some(UNib32(100)));

        // client 1 patches its property 2 again: later, so it wins
        level.apply(1, &cfg(vec![entry(2, on_change)], true, Some(300)), 3);
        assert_eq!(level.merged().entries[1], entry(2, on_change));
        assert_eq!(level.merged().budget, Some(UNib32(300)));

        assert!(level.remove(1));
        assert_eq!(
            level.merged().entries,
            [entry(2, periodic(50)), entry(3, on_change)]
        );
        // client 2 stops: nothing observed
        level.apply(2, &cfg(vec![], false, None), 4);
        assert!(level.entries.is_empty());
        assert!(level.merged().entries.is_empty());
    }
}

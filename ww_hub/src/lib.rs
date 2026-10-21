//! The ww hub (SRV-13): one process per user that holds WireWeaver devices and serves every program of the user over
//! a local socket. Design: tpm P2401 `design/ww-hub.md`; docs: `docs/transport/hub.md`.
//!
//! This crate is the hub as a library, so any program can host it: `ww hub serve`, or an app that calls
//! [main_if_hub] first in `main` so the client library can start the hub from the app's own executable (CLT-39,
//! `wire_weaver_client::hub`).
//!
//! Built so far (SRV-28): one hub per user (an OS lock on `hub.lock`, released on a crash), the socket in the runtime
//! dir, the `Preamble` of each connection picking the hub's API or a device, the API
//! itself (`ww_hub_api`: info, keep, shutdown, the registry of devices, holders and clients, events), exit after
//! [HubConfig::idle_exit] without connections. Device tunnels come with SRV-29: until then a device target is
//! answered `UnknownDevice`.

mod api;
pub mod client;
mod devices;

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use interprocess::local_socket::tokio::SendHalf;
use interprocess::local_socket::traits::tokio::Stream as _;
use tokio::io::AsyncWriteExt;
use tokio::sync::watch;
use tokio::time::Instant;
use tracing::{debug, info, warn};
use ww_local_socket::files::{HUB_LOCK, HUB_SOCKET};
use ww_local_socket::{Answer, Peer, Preamble, PreambleError, Status, Target};

/// `ww_hub` version: a newer hub replaces an older one (SRV-30).
pub const HUB_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Set by a client that starts the hub (see [main_if_hub]).
pub const SERVE_ENV: &str = wire_weaver_client::hub::SERVE_ENV;
/// Idle exit in seconds, overrides [HubConfig::idle_exit]'s default.
pub const IDLE_EXIT_ENV: &str = "WW_HUB_IDLE_EXIT";
/// Who started the hub, e.g. `dg (pid 4711)`, shown by `ww hub status`. Starters set it; without it the hub uses the
/// name of its parent process where it can find it.
pub const STARTED_BY_ENV: &str = "WW_HUB_STARTED_BY";

/// A client must send its preamble within this time, or it is dropped.
const PREAMBLE_TIMEOUT: Duration = Duration::from_secs(2);
/// How often the hub checks whether it is idle.
const IDLE_CHECK: Duration = Duration::from_millis(200);

#[derive(Clone, Debug)]
pub struct HubConfig {
    /// Where the socket and lock files are, [ww_local_socket::runtime_dir] normally.
    pub runtime_dir: PathBuf,
    /// Exit after this long without connections (and nothing holding it up, see `Hub::keep`).
    pub idle_exit: Duration,
}

impl HubConfig {
    /// The user's runtime dir, idle exit 5 min or [IDLE_EXIT_ENV].
    pub fn from_env() -> io::Result<Self> {
        let idle_exit = match std::env::var(IDLE_EXIT_ENV) {
            Ok(s) => Duration::from_secs(s.parse().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{IDLE_EXIT_ENV}={s}: expected seconds"),
                )
            })?),
            Err(_) => Duration::from_secs(300),
        };
        Ok(HubConfig {
            runtime_dir: ww_local_socket::runtime_dir()?,
            idle_exit,
        })
    }
}

/// Why [serve] returned.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Exit {
    /// Nobody used it for [HubConfig::idle_exit]
    Idle,
    /// Asked to (`Hub::shutdown`, [Shutdown::shutdown])
    Shutdown,
    /// Another hub runs for this user already
    AlreadyRunning,
}

/// Asks a running [serve] to exit, from the same process.
#[derive(Clone)]
pub struct Shutdown(watch::Sender<bool>);

impl Shutdown {
    pub fn shutdown(&self) {
        self.0.send_replace(true);
    }
}

/// If this process was started as the hub ([SERVE_ENV] set), serves until the hub exits and ends the process;
/// started with `--hub-version` (a client choosing the newest hub executable asks so), prints [HUB_VERSION] and
/// exits; returns otherwise, after telling `wire_weaver_client` that this executable can serve as the hub. Put it
/// first in `main` of an app that should be able to host the hub itself, so a user who has only that app installed
/// still gets one (CLT-39).
pub fn main_if_hub() {
    if std::env::args_os().nth(1).as_deref()
        == Some(std::ffi::OsStr::new(wire_weaver_client::hub::VERSION_ARG))
    {
        println!("{HUB_VERSION}");
        std::process::exit(0);
    }
    if std::env::var_os(SERVE_ENV).is_none() {
        wire_weaver_client::hub::register_self_hosting(HUB_VERSION);
        return;
    }
    let code = match run_blocking() {
        Ok(Exit::Idle | Exit::Shutdown | Exit::AlreadyRunning) => 0,
        Err(e) => {
            eprintln!("ww hub: {e}");
            1
        }
    };
    std::process::exit(code);
}

/// Serves on a new tokio runtime until the hub exits.
pub fn run_blocking() -> io::Result<Exit> {
    let config = HubConfig::from_env()?;
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(serve(config))
}

/// Runs the hub until it is idle for [HubConfig::idle_exit], is shut down, or finds another hub running.
pub async fn serve(config: HubConfig) -> io::Result<Exit> {
    match Bound::bind(config)? {
        Some(bound) => Ok(bound.run().await),
        None => Ok(Exit::AlreadyRunning),
    }
}

/// A hub that holds `hub.lock` and listens, not serving yet: [run](Self::run) does. Split from [serve] so a host
/// (tests, an app hosting the hub) knows it is up and can stop it.
pub struct Bound {
    config: HubConfig,
    _lock: File,
    socket: PathBuf,
    listener: ww_local_socket::Listener,
    hub: Arc<Hub>,
}

impl Bound {
    /// Takes `hub.lock` and listens; `None` when another hub runs for this user.
    pub fn bind(config: HubConfig) -> io::Result<Option<Bound>> {
        let Some(lock) = take_hub_lock(&config)? else {
            return Ok(None);
        };
        let socket = config.runtime_dir.join(HUB_SOCKET);
        // we hold hub.lock: a socket file there is left over from a hub that is gone
        let listener = ww_local_socket::listen(&socket, true)?;
        let (shutdown, _) = watch::channel(false);
        let hub = Arc::new(Hub {
            state: Mutex::new(State {
                clients: BTreeMap::new(),
                next_client: 1,
                connections: 0,
                last_active: Instant::now(),
                keep_until: None,
            }),
            shutdown,
            events: tokio::sync::broadcast::channel(256).0,
            started_ms: unix_ms(),
            started_by: started_by(),
            idle_exit: config.idle_exit,
            socket: socket.clone(),
        });
        Ok(Some(Bound {
            config,
            _lock: lock,
            socket,
            listener,
            hub,
        }))
    }

    pub fn shutdown_handle(&self) -> Shutdown {
        Shutdown(self.hub.shutdown.clone())
    }

    /// Serves until idle or shut down.
    pub async fn run(self) -> Exit {
        info!(
            "ww hub {HUB_VERSION} listening on {}",
            self.socket.display()
        );
        let mut shutdown = self.hub.shutdown.subscribe();
        let watcher = tokio::spawn(devices::watch(self.hub.clone()));
        let exit = loop {
            tokio::select! {
                accepted = self.listener.accept() => match accepted {
                    Ok((stream, peer)) => {
                        let hub = self.hub.clone();
                        tokio::spawn(async move { hub.connection(stream, peer).await });
                    }
                    Err(e) => {
                        warn!("ww hub: accept failed: {e}");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                },
                _ = async { shutdown.wait_for(|s| *s).await.is_ok() } => break Exit::Shutdown,
                _ = tokio::time::sleep(IDLE_CHECK) => {
                    if self.hub.idle_for() >= self.config.idle_exit {
                        break Exit::Idle;
                    }
                }
            }
        };
        watcher.abort();
        info!("ww hub exits: {exit:?}");
        // still holding hub.lock: nobody else can have bound the name since
        drop(self.listener);
        #[cfg(unix)]
        let _ = std::fs::remove_file(&self.socket);
        exit
    }
}

/// `hub.lock`, locked exclusively and with our pid written into it, or `None` when another hub holds it.
fn take_hub_lock(config: &HubConfig) -> io::Result<Option<File>> {
    let path = config.runtime_dir.join(HUB_LOCK);
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)?;
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Ok(None),
        Err(TryLockError::Error(e)) => return Err(e),
    }
    file.set_len(0)?;
    writeln!(
        file,
        "pid = {}\nversion = {HUB_VERSION}\nstarted = {}",
        std::process::id(),
        unix_ms()
    )?;
    file.flush()?;
    Ok(Some(file))
}

pub(crate) fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) struct Hub {
    pub(crate) state: Mutex<State>,
    shutdown: watch::Sender<bool>,
    /// `Registry::events` of every connection that opened the stream
    pub(crate) events: tokio::sync::broadcast::Sender<ww_hub_api::EventOwned>,
    pub(crate) started_ms: u64,
    pub(crate) started_by: String,
    pub(crate) idle_exit: Duration,
    pub(crate) socket: PathBuf,
}

pub(crate) struct State {
    pub(crate) clients: BTreeMap<u32, Client>,
    next_client: u32,
    /// Open connections of any kind; the hub is idle only without any
    connections: usize,
    /// When the last connection closed (or the hub started)
    last_active: Instant,
    /// `Hub::keep`: stay up until then
    keep_until: Option<Instant>,
}

/// A program connected to the hub's API: the OS's pid for it, and what its link setup said (CLT-41) once up.
#[derive(Clone, Debug)]
pub(crate) struct Client {
    pub(crate) peer_pid: Option<u32>,
    pub(crate) meta: Option<ww_device::PeerMeta>,
    pub(crate) since_ms: u64,
}

/// [STARTED_BY_ENV], else the parent process's name (Linux), else empty.
fn started_by() -> String {
    if let Ok(by) = std::env::var(STARTED_BY_ENV) {
        return by;
    }
    #[cfg(target_os = "linux")]
    {
        let ppid = std::os::unix::process::parent_id();
        if let Ok(comm) = std::fs::read_to_string(format!("/proc/{ppid}/comm")) {
            return format!("{} (pid {ppid})", comm.trim());
        }
    }
    String::new()
}

impl Hub {
    /// Seconds until the hub exits if nothing connects (`None`: it won't while connected or kept), and seconds left of
    /// `keep` (`None`: not kept).
    pub(crate) fn timers(&self) -> (u32, Option<u32>, Option<u32>, u32) {
        let state = self.state.lock().unwrap();
        let now = Instant::now();
        let keep_left = state
            .keep_until
            .filter(|k| *k > now)
            .map(|k| secs_ceil(k - now));
        let exits_in = if state.connections > 0 || keep_left.is_some() {
            None
        } else {
            Some(secs_ceil(
                self.idle_exit.saturating_sub(now - state.last_active),
            ))
        };
        (
            self.idle_exit.as_secs().min(u32::MAX as u64) as u32,
            exits_in,
            keep_left,
            state.connections as u32,
        )
    }

    fn idle_for(&self) -> Duration {
        let state = self.state.lock().unwrap();
        let now = Instant::now();
        if state.connections > 0 || state.keep_until.is_some_and(|k| k > now) {
            return Duration::ZERO;
        }
        let since = match state.keep_until {
            Some(k) => k.max(state.last_active),
            None => state.last_active,
        };
        now - since
    }

    async fn connection(self: Arc<Self>, stream: ww_local_socket::Stream, peer: Peer) {
        let _guard = ConnectionGuard::new(&self);
        let (mut rx, mut tx) = stream.split();
        let preamble = match tokio::time::timeout(PREAMBLE_TIMEOUT, Preamble::read(&mut rx)).await {
            Err(_) => {
                debug!("ww hub: {peer:?} sent no preamble");
                return;
            }
            Ok(Err(PreambleError::UnsupportedVersion(v))) => {
                debug!("ww hub: preamble version {v} from {peer:?}");
                let _ = answer(&mut tx, Status::PreambleVersion).await;
                return;
            }
            Ok(Err(e)) => {
                debug!("ww hub: bad preamble from {peer:?}: {e}");
                return;
            }
            Ok(Ok(p)) => p,
        };
        match preamble.target {
            Target::Hub => {
                if answer(&mut tx, Status::Ok).await.is_err() {
                    return;
                }
                let id = self.client_joined(peer);
                debug!("ww hub: client {id} ({peer:?}) connected");
                api::serve_connection(self.clone(), id, rx, tx).await;
                self.client_left(id);
                debug!("ww hub: client {id} left");
            }
            // TODO(SRV-29): tunnels to the devices the hub holds
            Target::Device(key) => {
                debug!("ww hub: {peer:?} asked for device {key:?}, none served yet");
                let _ = answer(&mut tx, Status::UnknownDevice).await;
            }
        }
    }

    fn client_joined(&self, peer: Peer) -> u32 {
        let mut state = self.state.lock().unwrap();
        let id = state.next_client;
        state.next_client += 1;
        state.clients.insert(
            id,
            Client {
                peer_pid: peer.pid,
                meta: None,
                since_ms: unix_ms(),
            },
        );
        id
    }

    fn client_left(&self, id: u32) {
        self.state.lock().unwrap().clients.remove(&id);
        let _ = self.events.send(ww_hub_api::EventOwned::ClientLeft { id });
    }

    /// The client's link came up: what it said about itself (CLT-41).
    pub(crate) fn client_up(&self, id: u32, meta: Option<ww_device::PeerMeta>) {
        let program = meta
            .as_ref()
            .map(|m| m.program.to_string())
            .unwrap_or_default();
        if let Some(c) = self.state.lock().unwrap().clients.get_mut(&id) {
            c.meta = meta;
        }
        let _ = self
            .events
            .send(ww_hub_api::EventOwned::ClientJoined { id, program });
    }

    pub(crate) fn keep(&self, seconds: u32) {
        let mut state = self.state.lock().unwrap();
        state.keep_until =
            (seconds > 0).then(|| Instant::now() + Duration::from_secs(seconds as u64));
    }

    pub(crate) fn request_shutdown(&self) {
        self.shutdown.send_replace(true);
    }
}

fn secs_ceil(d: Duration) -> u32 {
    (d.as_millis().div_ceil(1000)).min(u32::MAX as u128) as u32
}

async fn answer(tx: &mut SendHalf, status: Status) -> io::Result<()> {
    tx.write_all(&Answer::new(status, HUB_VERSION).encode())
        .await
}

/// Counts a connection while it lives, so the hub isn't idle meanwhile.
struct ConnectionGuard<'a>(&'a Hub);

impl<'a> ConnectionGuard<'a> {
    fn new(hub: &'a Hub) -> Self {
        hub.state.lock().unwrap().connections += 1;
        ConnectionGuard(hub)
    }
}

impl Drop for ConnectionGuard<'_> {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap();
        state.connections -= 1;
        state.last_active = Instant::now();
    }
}

#[cfg(test)]
mod tests;

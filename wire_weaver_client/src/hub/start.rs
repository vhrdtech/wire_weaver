//! Finding the hub, starting it when none runs (CLT-39).
//!
//! One hub per user holds `hub.lock` for its whole life ([hub_running]). A client that wants the hub and finds none
//! takes `hub.start.lock`, checks again (another client may have started one meanwhile), removes a socket file left
//! behind by a hub that is gone, starts the newest hub executable it can find ([candidates]) detached from itself,
//! and waits until the hub answers a preamble on its socket. Two clients starting at once end up with one hub: the
//! second waits on the start lock and finds the first one's hub.

use std::fmt;
use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use semver::Version;
use tracing::{debug, info};
use ww_local_socket::Target;
use ww_local_socket::files::{HUB_LOCK, HUB_LOG, HUB_SOCKET, HUB_START_LOCK};

use super::{EXE_ENV, HANDOVER_ENV, SERVE_ENV, VERSION_ARG};

/// How long a started hub has to answer on its socket.
const READY_TIMEOUT: Duration = Duration::from_secs(2);
/// How long a candidate executable has to print its hub version.
const VERSION_TIMEOUT: Duration = Duration::from_secs(2);
/// Waiting for another client that is starting a hub: its start plus its readiness wait.
const START_LOCK_TIMEOUT: Duration = Duration::from_secs(5);
/// One readiness probe: connect, preamble, answer.
const PROBE_TIMEOUT: Duration = Duration::from_millis(300);
const POLL: Duration = Duration::from_millis(20);

/// The hub version of this executable, when it can serve as the hub (`ww_hub::main_if_hub` registers it).
static SELF_HOSTING: OnceLock<&'static str> = OnceLock::new();

/// Says that this executable serves as the hub when started with [SERVE_ENV] set, with hub version `version`.
/// `ww_hub::main_if_hub()` calls it; a client then starts the hub from the app itself when it has no newer one.
pub fn register_self_hosting(version: &'static str) {
    let _ = SELF_HOSTING.set(version);
}

/// Why no hub is there to use.
#[derive(Debug)]
pub enum HubUnavailable {
    /// None runs, and none was to be started
    NotRunning,
    /// No hub executable found
    NoExecutable,
    /// The hub was started (or runs) but didn't answer on its socket in time
    NotReady {
        socket: PathBuf,
    },
    /// Starting it failed
    StartFailed {
        exe: PathBuf,
        why: String,
    },
    Io(io::Error),
}

impl fmt::Display for HubUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HubUnavailable::NotRunning => write!(f, "no ww hub runs"),
            HubUnavailable::NoExecutable => write!(
                f,
                "no ww hub executable found: install `ww` (cargo install wire_weaver_cli), set {EXE_ENV}, or call \
                 ww_hub::main_if_hub() first in main"
            ),
            HubUnavailable::NotReady { socket } => write!(
                f,
                "the ww hub doesn't answer on {} within {READY_TIMEOUT:?}",
                socket.display()
            ),
            HubUnavailable::StartFailed { exe, why } => {
                write!(f, "starting the ww hub ({}) failed: {why}", exe.display())
            }
            HubUnavailable::Io(e) => write!(f, "ww hub: {e}"),
        }
    }
}

impl std::error::Error for HubUnavailable {}

impl From<io::Error> for HubUnavailable {
    fn from(e: io::Error) -> Self {
        HubUnavailable::Io(e)
    }
}

/// Whether a hub runs for this user: someone holds `hub.lock`. Nothing is connected to.
pub fn hub_running() -> bool {
    ww_local_socket::runtime_dir().is_ok_and(|dir| lock_held(&dir.join(HUB_LOCK)))
}

fn lock_held(path: &Path) -> bool {
    let Ok(file) = File::open(path) else {
        return false;
    };
    match file.try_lock_shared() {
        Ok(()) => {
            let _ = file.unlock();
            false
        }
        Err(TryLockError::WouldBlock) => true,
        Err(TryLockError::Error(_)) => false,
    }
}

/// The socket of the user's hub once it answers there. With `start`, a hub is started when none runs; without, an
/// absent hub is [HubUnavailable::NotRunning].
pub async fn ensure_running(start: bool) -> Result<PathBuf, HubUnavailable> {
    let dir = ww_local_socket::runtime_dir()?;
    let socket = dir.join(HUB_SOCKET);
    if lock_held(&dir.join(HUB_LOCK)) {
        // it may have just taken the lock and not listen yet
        let running = wait_ready(&dir, &socket, None).await?;
        if start && let Some(running) = running {
            replace_older(&dir, &socket, &running).await;
        }
        return Ok(socket);
    }
    if !start {
        return Err(HubUnavailable::NotRunning);
    }
    let _start_lock = start_lock(&dir).await?;
    if lock_held(&dir.join(HUB_LOCK)) {
        debug!("ww hub: started by another program meanwhile");
        return wait_ready(&dir, &socket, None).await.map(|_| socket);
    }
    // nobody holds hub.lock: a socket file there is left over from a hub that is gone
    #[cfg(unix)]
    match std::fs::remove_file(&socket) {
        Ok(()) => debug!("ww hub: removed stale {}", socket.display()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let candidate = tokio::task::spawn_blocking(candidates)
        .await
        .map_err(|e| HubUnavailable::Io(io::Error::other(e)))?
        .into_iter()
        .next()
        .ok_or(HubUnavailable::NoExecutable)?;
    let mut child = spawn(&dir, &candidate, false)?;
    info!(
        "ww hub {} started from {} ({}), pid {}",
        candidate.version,
        candidate.exe.display(),
        candidate.source,
        child.id()
    );
    let ready = wait_ready(&dir, &socket, Some((&mut child, &candidate.exe))).await;
    // the hub outlives us or exits when idle: someone has to reap it
    std::thread::spawn(move || child.wait());
    ready.map(|_| socket)
}

/// Replaces the running hub of version `running` with this program's own when that is strictly newer (SRV-30): the
/// new one takes over (`Hub::handover`) and the clients of the old one reconnect to it (CLT-40). Only this program's
/// own hub is considered (registered by `ww_hub::main_if_hub()`): it costs no process to find out, so every connect
/// can check. A failure leaves the running hub in place and is logged.
async fn replace_older(dir: &Path, socket: &Path, running: &str) {
    let Some(ours) = SELF_HOSTING.get() else {
        return;
    };
    if !newer(ours, running) {
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    // as for a start: one program at a time, and no client starts a hub in the gap
    let _start_lock = match start_lock(dir).await {
        Ok(lock) => lock,
        Err(e) => {
            debug!("ww hub: not replacing {running}: {e}");
            return;
        }
    };
    // another program may have replaced it meanwhile
    match probe(socket).await {
        Some(v) if newer(ours, &v) => {}
        _ => return,
    }
    info!("ww hub {running} is older than this program's {ours}: replacing it");
    let candidate = Candidate {
        exe,
        version: Version::parse(ours).unwrap_or(Version::new(0, 0, 0)),
        source: "this program",
    };
    let mut child = match spawn(dir, &candidate, true) {
        Ok(child) => child,
        Err(e) => {
            info!("ww hub: replacing {running} failed: {e}");
            return;
        }
    };
    let deadline = Instant::now() + START_LOCK_TIMEOUT;
    loop {
        if probe(socket).await.as_deref() == Some(*ours) {
            info!("ww hub {ours} took over from {running}");
            break;
        }
        if let Ok(Some(status)) = child.try_wait() {
            info!(
                "ww hub: replacing {running} failed, the new hub exited with {status}, see {}",
                dir.join(HUB_LOG).display()
            );
            break;
        }
        if Instant::now() >= deadline {
            info!("ww hub: replacing {running} takes too long, going on with what answers");
            break;
        }
        tokio::time::sleep(POLL).await;
    }
    std::thread::spawn(move || child.wait());
}

/// `a` is a later version than `b`.
fn newer(a: &str, b: &str) -> bool {
    match (Version::parse(a), Version::parse(b)) {
        (Ok(a), Ok(b)) => a > b,
        _ => false,
    }
}

/// The version of the hub answering on `socket`, if one does.
async fn probe(socket: &Path) -> Option<String> {
    match tokio::time::timeout(
        PROBE_TIMEOUT,
        crate::local_socket::open(socket, Target::Hub),
    )
    .await
    {
        Ok(Ok(tunnel)) => Some(tunnel.hub_version),
        Ok(Err(crate::local_socket::OpenError::Refused { answer, .. })) => Some(answer.hub_version),
        _ => None,
    }
}

/// `hub.start.lock`, held until dropped.
async fn start_lock(dir: &Path) -> Result<File, HubUnavailable> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(HUB_START_LOCK))?;
    let deadline = Instant::now() + START_LOCK_TIMEOUT;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                tokio::time::sleep(POLL).await
            }
            Err(TryLockError::WouldBlock) => {
                return Err(HubUnavailable::Io(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "another program holds hub.start.lock and doesn't finish",
                )));
            }
            Err(TryLockError::Error(e)) => return Err(e.into()),
        }
    }
}

/// Polls the socket until the hub answers a preamble, [READY_TIMEOUT] at most; the hub's version. A started hub
/// that exits early fails it, unless another hub took `hub.lock` (two starters raced, the other one won).
async fn wait_ready(
    dir: &Path,
    socket: &Path,
    mut started: Option<(&mut Child, &PathBuf)>,
) -> Result<Option<String>, HubUnavailable> {
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        // any answer is a hub that listens; a refusal of the hub API itself (preamble version) is the tunnel's to
        // report
        if let Some(version) = probe(socket).await {
            return Ok(Some(version));
        }
        if let Some((child, exe)) = started.as_mut()
            && let Ok(Some(status)) = child.try_wait()
            && !lock_held(&dir.join(HUB_LOCK))
        {
            return Err(HubUnavailable::StartFailed {
                exe: exe.to_path_buf(),
                why: format!("exited with {status}, see {}", dir.join(HUB_LOG).display()),
            });
        }
        if Instant::now() >= deadline {
            return Err(HubUnavailable::NotReady {
                socket: socket.to_path_buf(),
            });
        }
        tokio::time::sleep(POLL).await;
    }
}

/// A hub executable and its hub version.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub exe: PathBuf,
    pub version: Version,
    /// Where it was found: `WW_HUB_EXE`, `this program`, `next to this program`, `PATH`
    pub source: &'static str,
}

/// Hub executables to start, best first: [EXE_ENV] alone when set; else this program if it registered itself
/// ([register_self_hosting]), `ww` next to it and `ww` on `PATH`, the newest hub version first (each asked with
/// [VERSION_ARG]; one that doesn't answer is left out).
pub fn candidates() -> Vec<Candidate> {
    if let Some(exe) = std::env::var_os(EXE_ENV).filter(|e| !e.is_empty()) {
        let exe = PathBuf::from(exe);
        let version = hub_version(&exe).unwrap_or(Version::new(0, 0, 0));
        return vec![Candidate {
            exe,
            version,
            source: "WW_HUB_EXE",
        }];
    }
    let current = std::env::current_exe().ok();
    let mut found: Vec<Candidate> = vec![];
    if let (Some(v), Some(exe)) = (SELF_HOSTING.get(), &current)
        && let Ok(version) = Version::parse(v)
    {
        found.push(Candidate {
            exe: exe.clone(),
            version,
            source: "this program",
        });
    }
    let ww = format!("ww{}", std::env::consts::EXE_SUFFIX);
    let mut others: Vec<(PathBuf, &'static str)> = vec![];
    if let Some(dir) = current.as_ref().and_then(|e| e.parent()) {
        others.push((dir.join(&ww), "next to this program"));
    }
    if let Some(path) = std::env::var_os("PATH") {
        others.extend(std::env::split_paths(&path).map(|d| (d.join(&ww), "PATH")));
    }
    let mut seen: Vec<PathBuf> = found
        .iter()
        .filter_map(|c| c.exe.canonicalize().ok())
        .collect();
    others.retain(|(exe, _)| {
        let Ok(real) = exe.canonicalize() else {
            return false;
        };
        if !real.is_file() || seen.contains(&real) {
            return false;
        }
        seen.push(real);
        true
    });
    let asked: Vec<_> = others
        .into_iter()
        .map(|(exe, source)| {
            std::thread::spawn(move || {
                hub_version(&exe).map(|version| Candidate {
                    exe,
                    version,
                    source,
                })
            })
        })
        .collect();
    found.extend(asked.into_iter().filter_map(|t| t.join().ok().flatten()));
    // stable: among equal versions the order above wins
    found.sort_by(|a, b| b.version.cmp(&a.version));
    found
}

/// Runs `exe --hub-version`, its answer if it gives one in time.
fn hub_version(exe: &Path) -> Option<Version> {
    let mut child = Command::new(exe)
        .arg(VERSION_ARG)
        .env_remove(SERVE_ENV)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + VERSION_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) if Instant::now() >= deadline => {
                debug!("{} doesn't answer {VERSION_ARG}", exe.display());
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    let mut out = String::new();
    io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
    Version::parse(out.trim()).ok()
}

/// Starts `candidate` as the hub, detached: its own process group (a Ctrl-C in the terminal doesn't reach it), no
/// console window on Windows, stdin and stdout closed, stderr into `hub.log` in the runtime dir.
/// With `handover`, it takes over from the running hub ([HANDOVER_ENV], SRV-30).
fn spawn(dir: &Path, candidate: &Candidate, handover: bool) -> Result<Child, HubUnavailable> {
    let log = File::create(dir.join(HUB_LOG))?;
    let mut cmd = Command::new(&candidate.exe);
    if handover {
        cmd.env(HANDOVER_ENV, "1");
    } else {
        cmd.env_remove(HANDOVER_ENV);
    }
    cmd.env(SERVE_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
    cmd.spawn().map_err(|e| HubUnavailable::StartFailed {
        exe: candidate.exe.clone(),
        why: e.to_string(),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Mutex;

    /// The runtime dir and the hub's variables come from the environment: tests that set them run one at a time.
    pub(crate) static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Runs `f` with a fresh runtime dir (and no hub variables set), restoring the environment after.
    pub(crate) fn with_runtime_dir<R>(f: impl FnOnce(&Path) -> R) -> R {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = std::env::temp_dir().join(format!("ww-hub-client-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let vars = [
            ww_local_socket::RUNTIME_DIR_ENV,
            super::super::MODE_ENV,
            super::super::OPENS_ENV,
            EXE_ENV,
        ];
        let saved: Vec<_> = vars.iter().map(std::env::var_os).collect();
        // SAFETY: tests that touch the environment hold ENV_LOCK
        unsafe {
            std::env::set_var(ww_local_socket::RUNTIME_DIR_ENV, &tmp);
            for v in &vars[1..] {
                std::env::remove_var(v);
            }
        }
        let result = f(&ww_local_socket::runtime_dir().unwrap());
        for (v, s) in vars.iter().zip(saved) {
            // SAFETY: as above
            unsafe {
                match s {
                    Some(s) => std::env::set_var(v, s),
                    None => std::env::remove_var(v),
                }
            }
        }
        let _ = std::fs::remove_dir_all(&tmp);
        result
    }

    #[test]
    fn no_hub_runs_in_a_fresh_dir() {
        with_runtime_dir(|_| assert!(!hub_running()));
    }

    #[test]
    fn a_held_hub_lock_is_a_running_hub() {
        with_runtime_dir(|dir| {
            let lock = File::create(dir.join(HUB_LOCK)).unwrap();
            lock.lock().unwrap();
            assert!(hub_running());
            drop(lock);
            assert!(!hub_running());
        });
    }

    #[test]
    fn an_executable_named_in_the_env_is_the_only_candidate() {
        with_runtime_dir(|dir| {
            let exe = dir.join("no-such-hub");
            // SAFETY: with_runtime_dir holds ENV_LOCK and restores the variable
            unsafe { std::env::set_var(EXE_ENV, &exe) };
            let found = candidates();
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].exe, exe);
        });
    }
}

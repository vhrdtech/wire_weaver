//! Who holds a device (CLT-38): a lock-file registry under the user's runtime dir,
//! `<runtime>/ww/holders/<key>.lock`, written by every holder (a direct-mode program today, the hub later).
//!
//! A holder takes an exclusive [`std::fs::File::try_lock`] on the device's lock file before claiming the USB
//! interface or RTT probe, and keeps the file open ([`Claim`]) for as long as it holds the device. The OS releases
//! the lock when the process dies (a crash, `kill -9`), so a stale claim never lingers: anyone checking the lock
//! (with a *different* file handle, as [`holder_of`] and [`claim`] do) only ever sees it held while the holder is
//! actually alive. The file's content (a holder's pid, program, mode and so on) can go stale when a holder
//! crashes without releasing cleanly, but that is harmless: content is only trusted while the lock itself is held,
//! never on its own.
//!
//! This module has no feature gate: direct-mode programs write the lock file too, not only a future hub.

#[cfg(feature = "local_socket")]
mod handover;
mod proc_scan;

#[cfg(all(feature = "usb", target_os = "linux"))]
pub(crate) use proc_scan::describe_holder_of_node;
pub use proc_scan::holder_of_usb_device;
#[cfg(all(feature = "usb", target_os = "linux"))]
pub(crate) use proc_scan::usb_device_node;

use std::fmt;
use std::fs::{File, TryLockError};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::client_meta::ClientMetaOwned;

/// Identifies the device a lock file is about, see [`key_for`].
pub enum DeviceRef<'a> {
    /// `usb:<vid>:<pid>:<serial>[:<iface>]`
    Usb {
        vid: u16,
        pid: u16,
        serial: &'a str,
        /// The WireWeaver interface number, once known (connect() finds it after opening the device).
        iface: Option<u8>,
    },
    /// `rtt:<probe serial>:<chip>`
    Rtt {
        probe_serial: &'a str,
        chip: &'a str,
    },
}

/// The stable key naming a device's lock file, the same for every holder of that device.
pub fn key_for(device: DeviceRef<'_>) -> String {
    match device {
        DeviceRef::Usb {
            vid,
            pid,
            serial,
            iface: Some(iface),
        } => format!("usb:{vid:04x}:{pid:04x}:{serial}:{iface}"),
        DeviceRef::Usb {
            vid,
            pid,
            serial,
            iface: None,
        } => format!("usb:{vid:04x}:{pid:04x}:{serial}"),
        DeviceRef::Rtt { probe_serial, chip } => format!("rtt:{probe_serial}:{chip}"),
    }
}

/// How a holder reached the device: directly, or (later) through the hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Direct,
    Hub,
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Mode::Direct => "direct",
            Mode::Hub => "hub",
        })
    }
}

impl Mode {
    fn parse(s: &str) -> Option<Mode> {
        match s {
            "direct" => Some(Mode::Direct),
            "hub" => Some(Mode::Hub),
            _ => None,
        }
    }
}

/// Who holds a device, read back from its lock file's content.
#[derive(Debug, Clone)]
pub struct Holder {
    pub pid: u32,
    /// Process name, e.g. `dg` (the file name of [`std::env::current_exe`]).
    pub program: String,
    /// Full path of the holder's executable.
    pub exe: String,
    pub mode: Mode,
    pub since: SystemTime,
    /// `wire_weaver_client`'s own version, so an old lock file from a pre-CLT-38 build never confuses a reader.
    pub ww_version: String,
    /// What the holder said about itself (CLT-41): `Cli`, `Gui`, `Notebook`, `Service`, `Hub`, `Test`, `Unknown`;
    /// empty when it sent nothing (or wrote the file before CLT-39).
    pub kind: String,
    /// Login name, empty if unknown.
    pub user: String,
    /// Host name, empty if unknown.
    pub host: String,
    /// A name the user gave the holder, empty if none.
    pub label: String,
    /// What the holder does with the device, e.g. "DFU update", empty if not said.
    pub purpose: String,
    /// Shared by every connection of one client object, 0 if unknown.
    pub instance_id: u64,
    /// The holder's socket for a polite handover to the hub (CLT-43), empty if it listens on none (a hub, a build
    /// without `local_socket`, a file written before CLT-43).
    pub handover: String,
}

impl Holder {
    fn current(mode: Mode, meta: Option<&ClientMetaOwned>) -> Holder {
        let exe = std::env::current_exe().unwrap_or_default();
        let program = match meta {
            Some(m) if !m.program.is_empty() => m.program.clone(),
            _ => exe
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "<unknown>".to_string()),
        };
        Holder {
            pid: std::process::id(),
            program,
            exe: exe.to_string_lossy().into_owned(),
            mode,
            since: SystemTime::now(),
            ww_version: env!("CARGO_PKG_VERSION").to_string(),
            kind: meta.map(|m| format!("{:?}", m.kind)).unwrap_or_default(),
            user: meta.map(|m| m.user.clone()).unwrap_or_default(),
            host: meta.map(|m| m.host.clone()).unwrap_or_default(),
            label: meta.map(|m| m.label.clone()).unwrap_or_default(),
            purpose: meta.and_then(|m| m.purpose.clone()).unwrap_or_default(),
            instance_id: meta.map(|m| m.instance_id).unwrap_or(0),
            handover: String::new(),
        }
    }

    /// A placeholder for a holder whose lock file content didn't parse (corrupted, or written by a future,
    /// incompatible format): the lock is still real, we just can't describe who holds it.
    fn unknown() -> Holder {
        Holder {
            pid: 0,
            program: "<unknown>".to_string(),
            exe: String::new(),
            mode: Mode::Direct,
            since: UNIX_EPOCH,
            ww_version: String::new(),
            kind: String::new(),
            user: String::new(),
            host: String::new(),
            label: String::new(),
            purpose: String::new(),
            instance_id: 0,
            handover: String::new(),
        }
    }

    fn serialize(key: &str, holder: &Holder) -> String {
        let since = holder
            .since
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        format!(
            "key = {key}\n\
             pid = {pid}\n\
             program = {program}\n\
             exe = {exe}\n\
             mode = {mode}\n\
             since = {since}\n\
             ww_version = {ww_version}\n\
             kind = {kind}\n\
             user = {user}\n\
             host = {host}\n\
             label = {label}\n\
             purpose = {purpose}\n\
             instance_id = {instance_id}\n\
             handover = {handover}\n",
            key = key,
            pid = holder.pid,
            program = holder.program,
            exe = holder.exe,
            mode = holder.mode,
            since = since,
            ww_version = holder.ww_version,
            kind = holder.kind,
            user = one_line(&holder.user),
            host = one_line(&holder.host),
            label = one_line(&holder.label),
            purpose = one_line(&holder.purpose),
            instance_id = holder.instance_id,
            handover = one_line(&holder.handover),
        )
    }

    /// Parses lock file content written by [`Holder::serialize`], returning the key too.
    fn parse_with_key(content: &str) -> Option<(String, Holder)> {
        let mut key = None;
        let mut pid = None;
        let mut program = None;
        let mut exe = None;
        let mut mode = None;
        let mut since = None;
        let mut ww_version = None;
        let mut extra = Holder::unknown();
        for line in content.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "key" => key = Some(v.to_string()),
                "pid" => pid = v.parse().ok(),
                "program" => program = Some(v.to_string()),
                "exe" => exe = Some(v.to_string()),
                "mode" => mode = Mode::parse(v),
                "since" => since = v.parse::<u64>().ok(),
                "ww_version" => ww_version = Some(v.to_string()),
                // added with CLT-39, absent in older files
                "kind" => extra.kind = v.to_string(),
                "user" => extra.user = v.to_string(),
                "host" => extra.host = v.to_string(),
                "label" => extra.label = v.to_string(),
                "purpose" => extra.purpose = v.to_string(),
                "instance_id" => extra.instance_id = v.parse().unwrap_or(0),
                // added with CLT-43
                "handover" => extra.handover = v.to_string(),
                _ => {}
            }
        }
        let holder = Holder {
            pid: pid?,
            program: program?,
            exe: exe?,
            mode: mode?,
            since: UNIX_EPOCH + std::time::Duration::from_secs(since?),
            ww_version: ww_version?,
            ..extra
        };
        Some((key?, holder))
    }

    fn parse(content: &str) -> Option<Holder> {
        Holder::parse_with_key(content).map(|(_, h)| h)
    }
}

impl fmt::Display for Holder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "held by {}", self.program)?;
        if !self.label.is_empty() {
            write!(f, " \"{}\"", self.label)?;
        }
        write!(f, ", pid {}", self.pid)?;
        if !self.kind.is_empty() && self.kind != "Unknown" {
            write!(f, ", {}", self.kind)?;
        }
        if !self.user.is_empty() {
            write!(f, ", {}", self.user)?;
            if !self.host.is_empty() {
                write!(f, "@{}", self.host)?;
            }
        }
        write!(
            f,
            ", {}, since {}",
            self.mode,
            format_time_of_day_utc(self.since)
        )?;
        if !self.purpose.is_empty() {
            write!(f, " ({})", self.purpose)?;
        }
        Ok(())
    }
}

/// `HH:MM:SS UTC`, the only calendar math this crate needs; see [`civil_from_unix_secs`] for the rest.
fn format_time_of_day_utc(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let s_of_day = secs % 86_400;
    let (h, m, s) = (s_of_day / 3600, (s_of_day / 60) % 60, s_of_day % 60);
    format!("{h:02}:{m:02}:{s:02} UTC")
}

/// Failure to claim a device's lock.
#[derive(Debug, thiserror::Error)]
pub enum ClaimError {
    /// Another WireWeaver program already holds this device.
    #[error("{0}")]
    HeldBy(Box<Holder>),
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// An exclusive claim on a device's lock file, kept open for as long as the device is held: the OS lock is
/// released the moment this (or the process) is dropped.
#[derive(Debug)]
pub struct Claim {
    /// Never read again: holding it open (and so the OS lock) for as long as `Claim` lives is the whole point.
    /// `None` only while dropping.
    file: Option<File>,
    key: String,
    /// Listening for askers of a handover to the hub (CLT-43)
    #[cfg(feature = "local_socket")]
    offer: Option<handover::Offer>,
}

impl Claim {
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Returns once another program asked this holder to hand the device to the hub and it allows it (CLT-43):
    /// the event loop then lets go of the device. Never for a claim that offers no handover. Cancel-safe.
    #[cfg_attr(not(any(feature = "usb", feature = "local_socket")), allow(dead_code))]
    pub(crate) async fn handover_requested(&mut self) {
        #[cfg(feature = "local_socket")]
        if let Some(offer) = self.offer.as_mut() {
            return offer.requested().await;
        }
        std::future::pending::<()>().await
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        // the socket goes while the lock is still ours, so it never takes away the next holder's socket
        #[cfg(feature = "local_socket")]
        if let Some(offer) = &self.offer {
            let _ = std::fs::remove_file(&offer.socket);
        }
        drop(self.file.take());
        // then a waiting asker hears that the device is free
        #[cfg(feature = "local_socket")]
        drop(self.offer.take());
    }
}

/// Takes the exclusive lock for `key`, writing this process's identity into its lock file, before claiming the
/// USB interface or RTT probe behind it. On [`ClaimError::HeldBy`], the caller never touches the device.
pub fn claim(key: &str, mode: Mode) -> Result<Claim, ClaimError> {
    claim_with(key, mode, None)
}

/// [`claim`], with what the client said about itself (CLT-41) in the lock file: kind, user, host, label, purpose.
pub fn claim_with(
    key: &str,
    mode: Mode,
    meta: Option<&ClientMetaOwned>,
) -> Result<Claim, ClaimError> {
    claim_offering(key, mode, meta, None)
}

/// [`claim_with`], listening for a polite handover to the hub (CLT-43) with `handover`: `Some(true)` hands the
/// device over when asked ([`Claim::handover_requested`]), `Some(false)` answers `Refused` with `meta`. Without a
/// tokio runtime or the `local_socket` feature it listens on nothing.
#[cfg_attr(not(any(feature = "usb", feature = "local_socket")), allow(dead_code))]
pub(crate) fn claim_offering(
    key: &str,
    mode: Mode,
    meta: Option<&ClientMetaOwned>,
    handover: Option<bool>,
) -> Result<Claim, ClaimError> {
    let path = lock_path(key)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)?;
    match file.try_lock() {
        Ok(()) => {
            let mut holder = Holder::current(mode, meta);
            #[cfg(feature = "local_socket")]
            let offer = handover.and_then(|allowed| {
                let dir = path.parent()?;
                handover::Offer::start(dir, key, allowed, meta)
                    .inspect_err(|e| tracing::debug!("{key}: no holder socket: {e}"))
                    .ok()
            });
            #[cfg(feature = "local_socket")]
            if let Some(offer) = &offer {
                holder.handover = offer.socket.display().to_string();
            }
            #[cfg(not(feature = "local_socket"))]
            let _ = (handover, &mut holder);
            let claim = Claim {
                file: Some(file),
                key: key.to_string(),
                #[cfg(feature = "local_socket")]
                offer,
            };
            let file = claim.file.as_ref().expect("just set");
            let mut file: &File = file;
            file.set_len(0)?;
            file.seek(SeekFrom::Start(0))?;
            file.write_all(Holder::serialize(key, &holder).as_bytes())?;
            file.flush()?;
            Ok(claim)
        }
        Err(TryLockError::WouldBlock) => Err(ClaimError::HeldBy(Box::new(read_holder(&mut file)?))),
        Err(TryLockError::Error(e)) => Err(ClaimError::Io(e)),
    }
}

/// Who currently holds `key`, if anyone; `None` means the device is free (no lock file, or one whose lock
/// nobody holds right now — a crashed holder's stale content is never reported as a live holder).
pub fn holder_of(key: &str) -> Option<Holder> {
    let path = lock_path(key).ok()?;
    let mut file = File::open(path).ok()?;
    match file.try_lock_shared() {
        Ok(()) => {
            let _ = file.unlock();
            None
        }
        Err(TryLockError::WouldBlock) => {
            Some(read_holder(&mut file).unwrap_or_else(|_| Holder::unknown()))
        }
        Err(TryLockError::Error(_)) => None,
    }
}

/// Every device currently held, `(key, holder)`, for `ww hub who` and friends. Lock files for devices nobody
/// holds right now are skipped; a registry directory that doesn't exist yet reads as empty.
pub fn list() -> Vec<(String, Holder)> {
    let dir = match holders_dir() {
        Ok(dir) => dir,
        Err(_) => return Vec::new(),
    };
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    let mut held = Vec::new();
    for entry in entries.flatten() {
        if entry.path().extension().and_then(|e| e.to_str()) != Some("lock") {
            continue;
        }
        let Ok(mut file) = File::open(entry.path()) else {
            continue;
        };
        match file.try_lock_shared() {
            Ok(()) => {
                let _ = file.unlock();
            }
            Err(TryLockError::WouldBlock) => {
                let mut content = String::new();
                if file.read_to_string(&mut content).is_ok()
                    && let Some((key, holder)) = Holder::parse_with_key(&content)
                {
                    held.push((key, holder));
                }
            }
            Err(TryLockError::Error(_)) => {}
        }
    }
    held.sort_by(|a, b| a.0.cmp(&b.0));
    held
}

fn read_holder(file: &mut File) -> io::Result<Holder> {
    file.seek(SeekFrom::Start(0))?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(Holder::parse(&content).unwrap_or_else(Holder::unknown))
}

/// Free text from a device or the user on one line, so it can't add a `key = value` line of its own.
fn one_line(s: &str) -> String {
    s.replace(['\n', '\r'], " ")
}

fn lock_path(key: &str) -> io::Result<PathBuf> {
    Ok(holders_dir()?.join(format!("{}.lock", sanitize_file_name(key))))
}

/// Replaces everything but ASCII letters, digits, `.`, `-` and `_` with `_`: device keys carry `:` (not valid in a
/// Windows file name) and USB string descriptors are untrusted input (a device could claim any serial number,
/// including one designed to look like a path), so nothing from a key reaches the filesystem unescaped.
pub(crate) fn sanitize_file_name(key: &str) -> String {
    key.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn holders_dir() -> io::Result<PathBuf> {
    // created 0700 and checked to be ours, as the runtime dir is (ww_local_socket::runtime_dir: the same dir the
    // hub's socket lives in, `WW_RUNTIME_DIR` included)
    let dir = ww_local_socket::runtime_dir()?.join(ww_local_socket::files::HOLDERS_DIR);
    match std::fs::create_dir(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::path::Path;
    #[cfg(unix)]
    use std::sync::Mutex;

    /// The runtime dir comes from an env var: tests run serially and restore it, so they don't fight the rest of
    /// the test binary (and each other) over `WW_RUNTIME_DIR`.
    #[cfg(unix)]
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[cfg(unix)]
    const RUNTIME_DIR_VAR: &str = ww_local_socket::RUNTIME_DIR_ENV;

    /// Points the runtime dir at a fresh temporary directory for the duration of `f`.
    #[cfg(unix)]
    fn with_runtime_dir<R>(f: impl FnOnce(&Path) -> R) -> R {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join(format!(
            "ww_holders_test_{}_{}",
            std::process::id(),
            unique()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let previous = std::env::var_os(RUNTIME_DIR_VAR);
        unsafe {
            std::env::set_var(RUNTIME_DIR_VAR, &tmp);
        }
        let result = f(&tmp);
        match previous {
            Some(p) => unsafe { std::env::set_var(RUNTIME_DIR_VAR, p) },
            None => unsafe { std::env::remove_var(RUNTIME_DIR_VAR) },
        }
        let _ = std::fs::remove_dir_all(&tmp);
        result
    }

    #[cfg(unix)]
    fn unique() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }

    #[test]
    fn key_for_usb_and_rtt() {
        assert_eq!(
            key_for(DeviceRef::Usb {
                vid: 0x046d,
                pid: 0xc52b,
                serial: "ABCD1234",
                iface: Some(2),
            }),
            "usb:046d:c52b:ABCD1234:2"
        );
        assert_eq!(
            key_for(DeviceRef::Usb {
                vid: 0x046d,
                pid: 0xc52b,
                serial: "ABCD1234",
                iface: None,
            }),
            "usb:046d:c52b:ABCD1234"
        );
        assert_eq!(
            key_for(DeviceRef::Rtt {
                probe_serial: "0671FF",
                chip: "STM32H725",
            }),
            "rtt:0671FF:STM32H725"
        );
    }

    #[test]
    fn holder_round_trips_through_content() {
        let meta = crate::ClientMeta::new()
            .kind(crate::ClientKind::Gui)
            .label("bench\n2")
            .purpose("calibration")
            .resolve();
        let holder = Holder::current(Mode::Direct, Some(&meta));
        let content = Holder::serialize("usb:1:2:s", &holder);
        let (key, parsed) = Holder::parse_with_key(&content).unwrap();
        assert_eq!(key, "usb:1:2:s");
        assert_eq!(parsed.pid, holder.pid);
        assert_eq!(parsed.program, holder.program);
        assert_eq!(parsed.mode, holder.mode);
        assert_eq!(parsed.kind, "Gui");
        assert_eq!(
            parsed.label, "bench 2",
            "a line break can't start a line of its own"
        );
        assert_eq!(parsed.purpose, "calibration");
        assert_eq!(parsed.instance_id, meta.instance_id);
        assert!(parsed.to_string().contains("Gui"), "{parsed}");
    }

    #[test]
    fn reads_a_file_written_before_client_meta() {
        let content = "key = usb:1:2:s\npid = 7\nprogram = dg\nexe = /bin/dg\nmode = direct\nsince = 1\nww_version = 0.5.0\n";
        let (_, h) = Holder::parse_with_key(content).unwrap();
        assert_eq!((h.pid, h.kind.as_str()), (7, ""));
    }

    #[test]
    fn sanitizes_path_hostile_serials() {
        assert_eq!(sanitize_file_name("usb:1:2:../../etc"), "usb_1_2_.._.._etc");
        assert!(!sanitize_file_name("a/b\\c").contains(['/', '\\']));
    }

    #[cfg(unix)]
    #[test]
    fn claim_then_claim_again_reports_the_first_holder() {
        with_runtime_dir(|_| {
            let key = format!("usb:test:{}", unique());
            let first = claim(&key, Mode::Direct).expect("first claim succeeds");
            let err = claim(&key, Mode::Direct).expect_err("second claim is refused");
            match err {
                ClaimError::HeldBy(holder) => {
                    assert_eq!(holder.pid, std::process::id());
                    assert_eq!(holder.mode, Mode::Direct);
                }
                ClaimError::Io(e) => panic!("expected HeldBy, got {e}"),
            }
            assert!(holder_of(&key).is_some());
            drop(first);
            assert!(holder_of(&key).is_none(), "released after drop");
        });
    }

    #[cfg(unix)]
    #[test]
    fn list_reports_only_currently_held_devices() {
        with_runtime_dir(|_| {
            let held_key = format!("usb:held:{}", unique());
            let free_key = format!("usb:free:{}", unique());
            let claim = self::claim(&held_key, Mode::Direct).unwrap();
            // claimed then released: its lock file lingers with stale content, but is free
            drop(self::claim(&free_key, Mode::Direct).unwrap());

            let held = list();
            assert!(held.iter().any(|(k, _)| k == &held_key));
            assert!(
                !held.iter().any(|(k, _)| k == &free_key),
                "a released claim must not show up as held, even with its stale file still on disk"
            );
            drop(claim);
        });
    }

    /// Name of the helper test below, as a `--exact` filter understood by the standard libtest harness this
    /// binary (whether run directly, via `cargo test`, or re-invoked like this from inside one of its own
    /// tests) implements regardless of which runner drives it.
    #[cfg(unix)]
    const HELPER_TEST_NAME: &str = "holders::tests::holders_test_claim_and_wait_helper";

    /// Spawns a child process that claims `key` and holds it (via `holders_test_claim_and_wait_helper` below,
    /// re-running this very binary filtered to that one test), returning once it has confirmed the claim.
    #[cfg(unix)]
    fn spawn_holding_child(key: &str) -> std::process::Child {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([HELPER_TEST_NAME, "--exact", "--ignored", "--nocapture"])
            .env("WW_HOLDERS_TEST_KEY", key)
            .env(RUNTIME_DIR_VAR, std::env::var_os(RUNTIME_DIR_VAR).unwrap())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("spawn holder helper");
        // the harness prints its own "running 1 test" line first; wait for the helper's own signal
        use std::io::BufRead;
        let stdout = std::io::BufReader::new(child.stdout.take().unwrap());
        for line in stdout.lines() {
            if line.unwrap_or_default().trim() == "holding" {
                break;
            }
        }
        child
    }

    /// Two real, separate holders of one device: a child process claims it, this process's own `claim()` is
    /// refused and names the child by pid and program (the generic "another program" message two independent
    /// `std::fs::File` locks could in principle give no further detail).
    #[cfg(unix)]
    #[test]
    fn two_holders_the_second_is_refused_and_named() {
        with_runtime_dir(|_| {
            let key = format!("usb:two_holders:{}", unique());
            let mut child = spawn_holding_child(&key);

            let err = claim(&key, Mode::Direct).expect_err("already held by the child");
            match err {
                ClaimError::HeldBy(holder) => {
                    assert_eq!(holder.pid, child.id());
                    assert_eq!(holder.mode, Mode::Direct);
                }
                ClaimError::Io(e) => panic!("expected HeldBy, got {e}"),
            }

            child.kill().expect("kill child");
            child.wait().unwrap();
        });
    }

    #[cfg(unix)]
    #[test]
    fn a_crashed_holder_releases_the_lock() {
        with_runtime_dir(|_| {
            let key = format!("usb:crash:{}", unique());
            let mut child = spawn_holding_child(&key);
            assert!(holder_of(&key).is_some(), "child should hold it");

            // simulate a crash: no cleanup, no unlock
            child.kill().expect("kill child");
            child.wait().unwrap();

            // the OS releases the lock the moment the process is gone; content may still be on disk
            for _ in 0..50 {
                if holder_of(&key).is_none() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            panic!("lock was not released after the holder was killed");
        });
    }

    /// Not a real test: `a_crashed_holder_releases_the_lock` re-invokes this binary filtered to exactly this
    /// name, which claims the lock named by `WW_HOLDERS_TEST_KEY`, prints `holding` and sleeps until the parent
    /// kills it. `#[ignore]` keeps it out of the normal `just test` run (an unfiltered run would otherwise sleep
    /// for a minute); the parent passes `--ignored` to run it anyway.
    #[test]
    #[ignore]
    fn holders_test_claim_and_wait_helper() {
        let key = std::env::var("WW_HOLDERS_TEST_KEY").expect("WW_HOLDERS_TEST_KEY");
        let _claim = claim(&key, Mode::Direct).expect("claim in helper");
        println!("holding");
        std::io::Write::flush(&mut std::io::stdout()).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}

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

mod proc_scan;

#[cfg(feature = "usb")]
pub(crate) use proc_scan::describe_holder_of_node;
pub use proc_scan::holder_of_usb_device;
#[cfg(all(feature = "usb", target_os = "linux"))]
pub(crate) use proc_scan::usb_device_node;

use std::fmt;
use std::fs::{File, TryLockError};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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
}

impl Holder {
    fn current(mode: Mode) -> Holder {
        let exe = std::env::current_exe().unwrap_or_default();
        let program = exe
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "<unknown>".to_string());
        Holder {
            pid: std::process::id(),
            program,
            exe: exe.to_string_lossy().into_owned(),
            mode,
            since: SystemTime::now(),
            ww_version: env!("CARGO_PKG_VERSION").to_string(),
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
             ww_version = {ww_version}\n",
            key = key,
            pid = holder.pid,
            program = holder.program,
            exe = holder.exe,
            mode = holder.mode,
            since = since,
            ww_version = holder.ww_version,
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
        };
        Some((key?, holder))
    }

    fn parse(content: &str) -> Option<Holder> {
        Holder::parse_with_key(content).map(|(_, h)| h)
    }
}

impl fmt::Display for Holder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "held by {}, pid {}, {}, since {}",
            self.program,
            self.pid,
            self.mode,
            format_time_of_day_utc(self.since)
        )
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
    HeldBy(Holder),
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// An exclusive claim on a device's lock file, kept open for as long as the device is held: the OS lock is
/// released the moment this (or the process) is dropped.
#[derive(Debug)]
pub struct Claim {
    /// Never read again: holding it open (and so the OS lock) for as long as `Claim` lives is the whole point.
    _file: File,
    key: String,
}

impl Claim {
    pub fn key(&self) -> &str {
        &self.key
    }
}

/// Takes the exclusive lock for `key`, writing this process's identity into its lock file, before claiming the
/// USB interface or RTT probe behind it. On [`ClaimError::HeldBy`], the caller never touches the device.
pub fn claim(key: &str, mode: Mode) -> Result<Claim, ClaimError> {
    let path = lock_path(key)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)?;
    match file.try_lock() {
        Ok(()) => {
            let holder = Holder::current(mode);
            file.set_len(0)?;
            file.seek(SeekFrom::Start(0))?;
            file.write_all(Holder::serialize(key, &holder).as_bytes())?;
            file.flush()?;
            Ok(Claim {
                _file: file,
                key: key.to_string(),
            })
        }
        Err(TryLockError::WouldBlock) => Err(ClaimError::HeldBy(read_holder(&mut file)?)),
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

fn lock_path(key: &str) -> io::Result<PathBuf> {
    Ok(holders_dir()?.join(format!("{}.lock", sanitize_file_name(key))))
}

/// Replaces everything but ASCII letters, digits, `.`, `-` and `_` with `_`: device keys carry `:` (not valid in a
/// Windows file name) and USB string descriptors are untrusted input (a device could claim any serial number,
/// including one designed to look like a path), so nothing from a key reaches the filesystem unescaped.
fn sanitize_file_name(key: &str) -> String {
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
    let dir = runtime_dir()?.join("holders");
    ensure_private_dir(&dir)?;
    Ok(dir)
}

/// `$XDG_RUNTIME_DIR/ww` on Linux, `$TMPDIR/ww` on macOS, `%LOCALAPPDATA%\ww\run` on Windows: created 0700 (or
/// checked to already be ours) the first time it's needed.
fn runtime_dir() -> io::Result<PathBuf> {
    let dir = base_runtime_dir()?;
    ensure_private_dir(&dir)?;
    Ok(dir)
}

fn base_runtime_dir() -> io::Result<PathBuf> {
    #[cfg(target_os = "windows")]
    let (var, suffix): (&str, &[&str]) = ("LOCALAPPDATA", &["ww", "run"]);
    #[cfg(target_os = "macos")]
    let (var, suffix): (&str, &[&str]) = ("TMPDIR", &["ww"]);
    #[cfg(all(unix, not(target_os = "macos")))]
    let (var, suffix): (&str, &[&str]) = ("XDG_RUNTIME_DIR", &["ww"]);

    let base = std::env::var_os(var)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{var} is not set")))?;
    let mut dir = PathBuf::from(base);
    dir.extend(suffix);
    Ok(dir)
}

/// Creates `dir` 0700 if missing, else refuses it if it isn't ours (same check `ssh ControlMaster`-style tools
/// skip and regret: a world-writable runtime dir would let another user plant a lock file or a socket).
fn ensure_private_dir(dir: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(dir) {
        Ok(meta) if meta.is_dir() => check_owned_by_us(dir, &meta),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} exists and is not a directory", dir.display()),
        )),
        Err(e) if e.kind() == io::ErrorKind::NotFound => create_private_dir(dir),
        Err(e) => Err(e),
    }
}

fn create_private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(unix)]
fn check_owned_by_us(dir: &Path, meta: &std::fs::Metadata) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let our_uid = rustix::process::getuid().as_raw();
    if meta.uid() != our_uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} is owned by uid {}, not us (uid {our_uid}); refusing to use it",
                dir.display(),
                meta.uid()
            ),
        ));
    }
    Ok(())
}

// TODO(CLT-38): check the owning SID on Windows once that platform gets a real test (SRV-33's local security
// pass covers the socket and pipe the same way).
#[cfg(windows)]
fn check_owned_by_us(_dir: &Path, _meta: &std::fs::Metadata) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// `runtime_dir()` reads an env var per OS: tests run serially and restore it, so they don't fight the rest
    /// of the test binary (and each other) over `XDG_RUNTIME_DIR`.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[cfg(target_os = "macos")]
    const RUNTIME_DIR_VAR: &str = "TMPDIR";
    #[cfg(all(unix, not(target_os = "macos")))]
    const RUNTIME_DIR_VAR: &str = "XDG_RUNTIME_DIR";

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
        let holder = Holder::current(Mode::Direct);
        let content = Holder::serialize("usb:1:2:s", &holder);
        let (key, parsed) = Holder::parse_with_key(&content).unwrap();
        assert_eq!(key, "usb:1:2:s");
        assert_eq!(parsed.pid, holder.pid);
        assert_eq!(parsed.program, holder.program);
        assert_eq!(parsed.mode, holder.mode);
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

//! Who this client is (CLT-41): program, user, host, pid, kind, sent to the device in link setup
//! ([ww_link::LinkSetup::client_meta]) so that a device log, a hub or a lock file can say who connected.
//!
//! Everything has a default found by the library (executable name, pid, login name, host name, kind from the parent
//! process) and can be overridden with [ClientMeta], set on a [ClientConfig](crate::ClientConfig).

use std::sync::OnceLock;

pub use ww_link::{ClientKind, ClientMetaOwned};

/// Longest `program`, `user`, `host` and `label` sent; longer ones are cut at a character boundary.
pub const MAX_NAME_BYTES: usize = 48;
/// Longest `app_version` sent.
pub const MAX_VERSION_BYTES: usize = 24;
/// Longest `purpose` sent.
pub const MAX_PURPOSE_BYTES: usize = 64;

/// What a client says about itself. Unset fields are filled in by the library, see [ClientMeta::resolve].
///
/// One value is created per [ClientConfig](crate::ClientConfig); clones of it keep the same
/// [instance id](Self::instance_id), so all connections of one client object show up as one.
#[derive(Clone, Debug)]
pub struct ClientMeta {
    program: Option<String>,
    app_version: Option<String>,
    label: Option<String>,
    kind: Option<ClientKind>,
    purpose: Option<String>,
    user: Option<String>,
    host: Option<String>,
    pid: Option<u32>,
    instance_id: u64,
}

impl Default for ClientMeta {
    fn default() -> Self {
        ClientMeta {
            program: None,
            app_version: None,
            label: None,
            kind: None,
            purpose: None,
            user: None,
            host: None,
            pid: None,
            instance_id: random_instance_id(),
        }
    }
}

impl ClientMeta {
    /// All defaults, with a new random [instance id](Self::instance_id).
    pub fn new() -> Self {
        Self::default()
    }

    /// Program name, default: file name of the running executable (without `.exe`).
    pub fn program(mut self, program: impl Into<String>) -> Self {
        self.program = Some(program.into());
        self
    }

    /// Version of the program (free text), default: empty.
    pub fn app_version(mut self, version: impl Into<String>) -> Self {
        self.app_version = Some(version.into());
        self
    }

    /// A name the user gave this client, default: empty.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Kind of program. Default: [ClientKind::Cli] if the parent process is a shell, else [ClientKind::Unknown];
    /// a GUI, service or test sets its own, `wire_weaver_py` sets [ClientKind::Notebook].
    pub fn kind(mut self, kind: ClientKind) -> Self {
        self.kind = Some(kind);
        self
    }

    /// What the program does with the device, e.g. "DFU update", default: nothing.
    pub fn purpose(mut self, purpose: impl Into<String>) -> Self {
        self.purpose = Some(purpose.into());
        self
    }

    /// Login name, default: from the environment (`USER`, `LOGNAME`, `USERNAME`).
    pub fn user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }

    /// Host name, default: the PC's host name.
    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.host = Some(host.into());
        self
    }

    /// Process id, default: this process.
    pub fn pid(mut self, pid: u32) -> Self {
        self.pid = Some(pid);
        self
    }

    /// Random number picked when this value was created (non-zero), shared by every connection made with a clone of
    /// it: tells clients of the same program and pid apart, and ties one client's several devices together.
    pub fn instance_id(&self) -> u64 {
        self.instance_id
    }

    /// The meta to send: set fields as they are, the rest detected (once per process), all cut to
    /// [MAX_NAME_BYTES] and friends.
    pub fn resolve(&self) -> ClientMetaOwned {
        let d = detected();
        ClientMetaOwned {
            program: cut(
                self.program.as_deref().unwrap_or(&d.program),
                MAX_NAME_BYTES,
            ),
            app_version: cut(self.app_version.as_deref().unwrap_or(""), MAX_VERSION_BYTES),
            label: cut(self.label.as_deref().unwrap_or(""), MAX_NAME_BYTES),
            kind: self.kind.unwrap_or(d.kind),
            pid: self.pid.unwrap_or_else(std::process::id),
            user: cut(self.user.as_deref().unwrap_or(&d.user), MAX_NAME_BYTES),
            host: cut(self.host.as_deref().unwrap_or(&d.host), MAX_NAME_BYTES),
            purpose: self.purpose.as_deref().map(|p| cut(p, MAX_PURPOSE_BYTES)),
            instance_id: self.instance_id,
        }
    }
}

/// The wire form of `meta`, borrowing its strings.
pub(crate) fn as_wire(meta: &ClientMetaOwned) -> ww_link::ClientMeta<'_> {
    ww_link::ClientMeta {
        program: &meta.program,
        app_version: &meta.app_version,
        label: &meta.label,
        kind: meta.kind,
        pid: meta.pid,
        user: &meta.user,
        host: &meta.host,
        purpose: meta.purpose.as_deref(),
        instance_id: meta.instance_id,
    }
}

/// `meta` with only what identifies the process, for a device with a small receive buffer.
pub(crate) fn shortened(meta: &ClientMetaOwned) -> ClientMetaOwned {
    ClientMetaOwned {
        program: cut(&meta.program, 16),
        app_version: String::new(),
        label: String::new(),
        kind: meta.kind,
        pid: meta.pid,
        user: cut(&meta.user, 16),
        host: cut(&meta.host, 16),
        purpose: None,
        instance_id: meta.instance_id,
    }
}

/// What [ClientConfig::client_meta](crate::ClientConfig::client_meta) holds.
#[derive(Clone, Debug)]
pub(crate) enum ClientMetaSetting {
    /// Send [ClientMeta::resolve]d.
    On(ClientMeta),
    /// Send nothing ([ClientConfig::no_client_meta](crate::ClientConfig::no_client_meta)).
    Off,
}

impl Default for ClientMetaSetting {
    fn default() -> Self {
        ClientMetaSetting::On(ClientMeta::default())
    }
}

/// Defaults found on the system, the same for every client of this process.
struct Detected {
    program: String,
    user: String,
    host: String,
    kind: ClientKind,
}

fn detected() -> &'static Detected {
    static DETECTED: OnceLock<Detected> = OnceLock::new();
    DETECTED.get_or_init(|| Detected {
        program: program_name(),
        user: user_name(),
        host: host_name(),
        kind: if sys::parent_is_shell() {
            ClientKind::Cli
        } else {
            ClientKind::Unknown
        },
    })
}

fn program_name() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_default()
}

fn user_name() -> String {
    ["USER", "LOGNAME", "USERNAME"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .unwrap_or_default()
}

fn host_name() -> String {
    sys::host_name().unwrap_or_default()
}

/// At most `max` bytes, not splitting a character.
fn cut(s: &str, max: usize) -> String {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

/// Names of shells whose children are commands typed by a person.
const SHELLS: [&str; 10] = [
    "bash",
    "zsh",
    "fish",
    "sh",
    "dash",
    "nu",
    "pwsh",
    "powershell",
    "cmd",
    "ksh",
];

fn is_shell(process_name: &str) -> bool {
    let name = process_name.trim().to_ascii_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    SHELLS.contains(&name)
}

fn random_instance_id() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    // RandomState is seeded randomly by the OS per process and per instance, no new dependency needed
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u32(std::process::id());
    if let Ok(t) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        h.write_u128(t.as_nanos());
    }
    h.finish().max(1)
}

#[cfg(target_os = "linux")]
mod sys {
    pub(super) fn host_name() -> Option<String> {
        let name = std::fs::read_to_string("/proc/sys/kernel/hostname").ok()?;
        Some(name.trim().to_owned())
    }

    pub(super) fn parent_is_shell() -> bool {
        // /proc/self/stat: "pid (comm) state ppid ...", comm may hold spaces and parentheses
        let Ok(stat) = std::fs::read_to_string("/proc/self/stat") else {
            return false;
        };
        let Some(after_comm) = stat.rsplit_once(')').map(|(_, rest)| rest) else {
            return false;
        };
        let Some(ppid) = after_comm.split_whitespace().nth(1) else {
            return false;
        };
        std::fs::read_to_string(format!("/proc/{ppid}/comm")).is_ok_and(|c| super::is_shell(&c))
    }
}

#[cfg(target_os = "macos")]
mod sys {
    pub(super) fn host_name() -> Option<String> {
        let mut buf = [0u8; 256];
        // SAFETY: buf is valid for buf.len() bytes
        let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
        if rc != 0 {
            return None;
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        Some(String::from_utf8_lossy(&buf[..end]).into_owned())
    }

    pub(super) fn parent_is_shell() -> bool {
        // SAFETY: getppid has no preconditions
        let ppid = unsafe { libc::getppid() };
        let mut buf = [0u8; 256];
        // SAFETY: buf is valid for buf.len() bytes
        let n = unsafe { libc::proc_name(ppid, buf.as_mut_ptr().cast(), buf.len() as u32) };
        n > 0 && super::is_shell(&String::from_utf8_lossy(&buf[..n as usize]))
    }
}

#[cfg(windows)]
mod sys {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    pub(super) fn host_name() -> Option<String> {
        std::env::var("COMPUTERNAME").ok().filter(|v| !v.is_empty())
    }

    /// (parent pid, exe name) of every process, from a Toolhelp snapshot.
    fn processes() -> Vec<(u32, u32, String)> {
        let mut out = Vec::new();
        // SAFETY: plain Win32 calls on a snapshot handle closed below; the entry is zeroed with dwSize set as the
        // API requires
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return out;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut ok = Process32FirstW(snap, &mut entry) != 0;
            while ok {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                out.push((
                    entry.th32ProcessID,
                    entry.th32ParentProcessID,
                    String::from_utf16_lossy(&entry.szExeFile[..len]),
                ));
                ok = Process32NextW(snap, &mut entry) != 0;
            }
            CloseHandle(snap);
        }
        out
    }

    pub(super) fn parent_is_shell() -> bool {
        let all = processes();
        let me = std::process::id();
        let Some(ppid) = all.iter().find(|p| p.0 == me).map(|p| p.1) else {
            return false;
        };
        all.iter()
            .find(|p| p.0 == ppid)
            .is_some_and(|p| super::is_shell(&p.2))
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod sys {
    pub(super) fn host_name() -> Option<String> {
        std::env::var("HOSTNAME").ok().filter(|v| !v.is_empty())
    }

    pub(super) fn parent_is_shell() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_filled() {
        let m = ClientMeta::new().resolve();
        assert!(!m.program.is_empty(), "the test binary's name");
        assert_eq!(m.pid, std::process::id());
        assert_ne!(m.instance_id, 0);
        assert_eq!(m.purpose, None);
        #[cfg(target_os = "linux")]
        assert!(!m.host.is_empty());
    }

    #[test]
    fn overrides_win_and_are_cut() {
        let long = "ä".repeat(100);
        let m = ClientMeta::new()
            .program("bench")
            .app_version("1.2.3")
            .label(long.clone())
            .kind(ClientKind::Gui)
            .purpose("dfu")
            .user("anna")
            .host("pc7")
            .pid(5)
            .resolve();
        assert_eq!(
            (m.program.as_str(), m.user.as_str(), m.host.as_str()),
            ("bench", "anna", "pc7")
        );
        assert_eq!(
            (m.kind, m.pid, m.purpose.as_deref()),
            (ClientKind::Gui, 5, Some("dfu"))
        );
        assert!(m.label.len() <= MAX_NAME_BYTES && m.label.chars().all(|c| c == 'ä'));
    }

    #[test]
    fn one_instance_id_per_client_object() {
        let a = ClientMeta::new();
        assert_eq!(a.clone().resolve().instance_id, a.resolve().instance_id);
        assert_ne!(
            ClientMeta::new().instance_id(),
            ClientMeta::new().instance_id()
        );
    }

    #[test]
    fn shell_names() {
        for s in [
            "bash\n", "zsh", "fish", "pwsh.exe", "CMD.EXE", "nu", "dash", "sh",
        ] {
            assert!(is_shell(s), "{s}");
        }
        for s in ["code", "python3", "systemd", "tmux: server", "explorer.exe"] {
            assert!(!is_shell(s), "{s}");
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parent_detection_reads_proc() {
        // the parent of a test is cargo-nextest or a shell, whichever: it must just not panic
        let _ = sys::parent_is_shell();
    }
}

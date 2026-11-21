use std::io;
use std::path::Path;

use interprocess::local_socket::tokio::prelude::*;
use interprocess::local_socket::{ListenerOptions, Name};
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};
use tracing::warn;

use crate::policy::PeerPolicy;

/// The socket as `interprocess` gives it, with tokio's IO traits.
type Inner = interprocess::local_socket::tokio::Stream;

/// A connection, a futures-io stream like every byte stream WireWeaver runs on (`ww_byte_stream`).
pub type Stream = Compat<Inner>;
/// The read half of a [Stream] (`futures_util::io::AsyncReadExt::split`).
pub type ReadHalf = futures_util::io::ReadHalf<Stream>;
/// The write half of a [Stream].
pub type WriteHalf = futures_util::io::WriteHalf<Stream>;

/// Who is on the other side of an accepted or opened connection, as the OS tells it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Peer {
    /// Process id, where the OS tells it (Linux, Windows; macOS doesn't)
    pub pid: Option<u32>,
    /// Effective user id of the other side.
    #[cfg(unix)]
    pub uid: u32,
    /// Effective group id of the other side (supplementary groups are not told).
    #[cfg(unix)]
    pub gid: u32,
}

/// Listens on `path` (a socket file in [runtime_dir](crate::runtime_dir), e.g. `hub.sock`; on Windows the named pipe
/// derived from it) for the same user only: [listen_with] and [PeerPolicy::SameUser].
pub fn listen(path: &Path, replace: bool) -> io::Result<Listener> {
    listen_with(path, replace, PeerPolicy::SameUser)
}

/// Listens on `path` for the users `policy` takes (SRV-46). The socket file gets the policy's mode and group
/// ([PeerPolicy::socket_mode]): bound at 0600 where the OS allows to set it before binding (Linux; elsewhere it is
/// set right after, and the directory guards it meanwhile), then given the group and widened to the policy's mode; its directory is the caller's, made with [dir_with](crate::dir_with)
/// for a policy other than the same user. On Windows the pipe's DACL gives access to its owner only and denies
/// network logons. [Listener::accept] checks every peer against the policy again.
///
/// `replace`: remove a socket file left behind by a process that is gone. The caller must know it is gone (the hub
/// holds `hub.lock` for that): a live listener's file would be taken away from it.
///
/// # Errors
///
/// Binding failed, the group can't be set (this user is not a member), or the policy is not
/// [PeerPolicy::SameUser] on Windows ([io::ErrorKind::Unsupported]).
pub fn listen_with(path: &Path, replace: bool, policy: PeerPolicy) -> io::Result<Listener> {
    let name = name_for(path)?;
    let opts = ListenerOptions::new()
        .name(name.clone())
        .try_overwrite(replace);
    let (inner, mode_set) = match create(opts, &policy) {
        Err(e) if e.kind() == io::ErrorKind::Unsupported && cfg!(unix) => {
            // no socket file mode before binding on this OS (macOS): set right after, the directory guards it
            let inner = ListenerOptions::new()
                .name(name)
                .try_overwrite(replace)
                .create_tokio()?;
            (inner, false)
        }
        r => (r?, true),
    };
    #[cfg(unix)]
    set_mode(path, &policy, mode_set)?;
    #[cfg(windows)]
    let _ = mode_set;
    Ok(Listener::new(inner, policy))
}

/// The policy's group, then its mode, on a socket file bound at 0600 (`mode_set`, narrowed further by the umask) or
/// with the umask's mode: never a moment with more bits than the policy's, nor group bits for the wrong group.
#[cfg(unix)]
fn set_mode(path: &Path, policy: &PeerPolicy, mode_set: bool) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let group = policy.group();
    if !mode_set {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    if let Some(group) = group {
        std::os::unix::fs::chown(path, None, Some(group))?;
    }
    if policy.socket_mode() != 0o600 {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(policy.socket_mode()))?;
    }
    Ok(())
}

/// Binds at 0600 whatever the policy: the umask narrows the mode bind gives, so [set_mode] widens it after.
#[cfg(unix)]
fn create(
    opts: ListenerOptions<'_>,
    _policy: &PeerPolicy,
) -> io::Result<interprocess::local_socket::tokio::Listener> {
    use interprocess::os::unix::local_socket::ListenerOptionsExt;
    opts.mode(0o600).create_tokio()
}

#[cfg(windows)]
fn create(
    opts: ListenerOptions<'_>,
    policy: &PeerPolicy,
) -> io::Result<interprocess::local_socket::tokio::Listener> {
    use interprocess::os::windows::local_socket::ListenerOptionsExt;
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;
    if *policy != PeerPolicy::SameUser {
        // TODO(SRV-71): other users of a pipe, by their SID
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "only the same-user policy on Windows",
        ));
    }
    // Protected DACL: deny network logons (NU), full access for the owner (OW, the user that creates the pipe),
    // nobody else. The default DACL would give Everyone read access.
    let sddl = widestring::U16CString::from_str("D:P(D;;GA;;;NU)(A;;GA;;;OW)")
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
    let sd = SecurityDescriptor::deserialize(&sddl)?;
    opts.security_descriptor(sd).create_tokio()
}

/// Opens a connection to a [listen]ing process of the same user, refusing one that runs as another user (a name
/// squatted by someone else).
pub async fn connect(path: &Path) -> io::Result<(Stream, Peer)> {
    let stream = Inner::connect(name_for(path)?).await?;
    let peer = check_peer(&stream, &PeerPolicy::SameUser)?;
    Ok((stream.compat(), peer))
}

/// Opens a connection to a service that runs as user `server_uid` (SRV-46), refusing one that runs as anyone else:
/// a service of another user listens with [listen_with] and a [PeerPolicy] that takes this user.
#[cfg(unix)]
pub async fn connect_to(path: &Path, server_uid: u32) -> io::Result<(Stream, Peer)> {
    let stream = Inner::connect(name_for(path)?).await?;
    let peer = peer_of(&stream)?;
    if peer.uid != server_uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("the other side runs as uid {}, not {server_uid}", peer.uid),
        ));
    }
    Ok((stream.compat(), peer))
}

pub struct Listener {
    inner: interprocess::local_socket::tokio::Listener,
    policy: PeerPolicy,
}

impl Listener {
    fn new(inner: interprocess::local_socket::tokio::Listener, policy: PeerPolicy) -> Self {
        Listener { inner, policy }
    }

    /// A listener on a socket made elsewhere ([inherited](crate::inherited)).
    #[cfg(unix)]
    pub(crate) fn from_inner(
        inner: interprocess::local_socket::tokio::Listener,
        policy: PeerPolicy,
    ) -> Self {
        Listener::new(inner, policy)
    }

    /// Who this listener takes.
    pub fn policy(&self) -> &PeerPolicy {
        &self.policy
    }

    /// Next connection of a user the policy takes. Connections of other users (only possible if the OS let them
    /// through the permissions) are dropped with a warning and not returned.
    pub async fn accept(&self) -> io::Result<(Stream, Peer)> {
        loop {
            let stream = self.inner.accept().await?;
            match check_peer(&stream, &self.policy) {
                Ok(peer) => return Ok((stream.compat(), peer)),
                Err(e) => warn!("local socket: dropped a connection: {e}"),
            }
        }
    }
}

/// Unix: the socket file itself. Windows: a pipe in the local namespace, named after the path so that two runtime
/// directories (tests, [RUNTIME_DIR_ENV](crate::RUNTIME_DIR_ENV)) don't share pipes.
fn name_for(path: &Path) -> io::Result<Name<'static>> {
    #[cfg(unix)]
    {
        use interprocess::local_socket::GenericFilePath;
        path.to_path_buf().to_fs_name::<GenericFilePath>()
    }
    #[cfg(windows)]
    {
        use interprocess::local_socket::GenericNamespaced;
        let file = path.file_stem().and_then(|f| f.to_str()).unwrap_or("ww");
        format!(
            "ww-{file}-{:016x}",
            fnv1a(path.as_os_str().as_encoded_bytes())
        )
        .to_ns_name::<GenericNamespaced>()
    }
}

#[cfg(windows)]
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The peer of `stream` as the kernel tells it.
#[cfg(unix)]
fn peer_of(stream: &Inner) -> io::Result<Peer> {
    let creds = stream.peer_creds()?;
    match (creds.euid(), creds.egid()) {
        (Some(uid), Some(gid)) => Ok(Peer {
            pid: creds.pid().map(|p| p as u32),
            uid,
            gid,
        }),
        _ => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            crate::policy::Refused::Unknown.to_string(),
        )),
    }
}

#[cfg(unix)]
fn check_peer(stream: &Inner, policy: &PeerPolicy) -> io::Result<Peer> {
    let peer = peer_of(stream)?;
    policy
        .admits(crate::dir::euid(), Some(peer.uid))
        .map_err(|e| io::Error::new(io::ErrorKind::PermissionDenied, e))?;
    Ok(peer)
}

#[cfg(windows)]
fn check_peer(stream: &Inner, policy: &PeerPolicy) -> io::Result<Peer> {
    // listen_with makes no other listener on Windows
    debug_assert!(
        *policy == PeerPolicy::SameUser,
        "a Windows listener is same-user only"
    );
    let pid = stream.peer_creds()?.pid();
    let Some(pid) = pid else {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the OS doesn't tell who is on the other side",
        ));
    };
    if !windows_user::same_user(pid)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("process {pid} on the other side runs as another user"),
        ));
    }
    Ok(Peer { pid: Some(pid) })
}

#[cfg(windows)]
mod windows_user {
    use std::io;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        EqualSid, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    /// Whether process `pid` runs as the same user (token user SID) as this one.
    pub fn same_user(pid: u32) -> io::Result<bool> {
        // SAFETY: plain Win32 calls; every handle opened here is closed by Handle's Drop, buffers are sized by the
        // first GetTokenInformation call and outlive the SID pointers taken from them.
        unsafe {
            let other = Handle(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid));
            if other.0.is_null() {
                return Err(io::Error::last_os_error());
            }
            let mine = token_user(GetCurrentProcess())?;
            let theirs = token_user(other.0)?;
            let sid = |b: &Vec<u8>| (*(b.as_ptr() as *const TOKEN_USER)).User.Sid;
            Ok(EqualSid(sid(&mine), sid(&theirs)) != 0)
        }
    }

    struct Handle(HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: a handle this module opened and nobody else closes
                unsafe { CloseHandle(self.0) };
            }
        }
    }

    unsafe fn token_user(process: HANDLE) -> io::Result<Vec<u8>> {
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: see same_user
        unsafe {
            if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
                return Err(io::Error::last_os_error());
            }
            let token = Handle(token);
            let mut len = 0u32;
            GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut len);
            // u64 words: TOKEN_USER holds pointers, the buffer must be aligned for them
            let mut buf = vec![0u64; (len as usize).div_ceil(8)];
            if GetTokenInformation(token.0, TokenUser, buf.as_mut_ptr().cast(), len, &mut len) == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(buf.iter().flat_map(|w| w.to_ne_bytes()).collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use futures_util::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    #[tokio::test]
    async fn same_user_connects_both_ways() {
        let dir = std::env::temp_dir().join(format!("ww-sock-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.sock");
        let listener = listen(&path, true).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            #[cfg(target_os = "linux")]
            assert_eq!(mode & 0o777, 0o600, "socket file is user-only");
            let _ = mode;
        }
        let server = tokio::spawn(async move {
            let (mut s, peer) = listener.accept().await.unwrap();
            assert_eq!(
                peer.pid,
                cfg!(any(target_os = "linux", windows)).then_some(std::process::id())
            );
            let mut b = [0u8; 4];
            s.read_exact(&mut b).await.unwrap();
            s.write_all(&b).await.unwrap();
        });
        let (mut c, peer) = connect(&path).await.unwrap();
        #[cfg(target_os = "linux")]
        assert_eq!(peer.pid, Some(std::process::id()));
        let _ = peer;
        c.write_all(b"ping").await.unwrap();
        let mut b = [0u8; 4];
        c.read_exact(&mut b).await.unwrap();
        assert_eq!(&b, b"ping");
        server.await.unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The socket file gets the policy's mode and group, and a peer of this user is taken under every policy, its
    /// uid and gid as the kernel tells them (SO_PEERCRED on Linux).
    #[cfg(unix)]
    #[tokio::test]
    async fn socket_modes_follow_the_policy() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = std::env::temp_dir().join(format!("ww-sock-policy-{}", std::process::id()));
        // SAFETY: getegid has no preconditions and cannot fail
        let group = unsafe { libc::getegid() };
        let me = crate::dir::euid();
        for policy in [
            PeerPolicy::SameUser,
            PeerPolicy::Users {
                group,
                uids: vec![],
            },
            PeerPolicy::Users {
                group,
                uids: vec![me.wrapping_add(1)],
            },
            PeerPolicy::Any,
        ] {
            crate::dir_with(&dir, &policy).unwrap();
            let path = dir.join("t.sock");
            let listener = listen_with(&path, true, policy.clone()).unwrap();
            let meta = std::fs::metadata(&path).unwrap();
            assert_eq!(
                meta.permissions().mode() & 0o777,
                policy.socket_mode(),
                "{policy:?}"
            );
            if let Some(group) = policy.group() {
                assert_eq!(meta.gid(), group, "{policy:?}");
            }
            let (accepted, connected) = tokio::join!(listener.accept(), connect_to(&path, me));
            let (_, peer) = accepted.unwrap();
            assert_eq!((peer.uid, peer.gid), (me, group), "{policy:?}");
            #[cfg(target_os = "linux")]
            assert_eq!(peer.pid, Some(std::process::id()));
            assert_eq!(connected.unwrap().1.uid, me);
            drop(listener);
        }
        // a service is not taken for another user's
        let path = dir.join("t.sock");
        let _listener = listen_with(&path, true, PeerPolicy::Any).unwrap();
        let refused = connect_to(&path, me.wrapping_add(1)).await;
        assert!(
            matches!(&refused, Err(e) if e.kind() == io::ErrorKind::PermissionDenied),
            "refused"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn live_socket_not_replaced_without_asking() {
        let dir = std::env::temp_dir().join(format!("ww-sock-busy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.sock");
        let _first = listen(&path, true).unwrap();
        let e = listen(&path, false).err().expect("second listener refused");
        assert_eq!(e.kind(), io::ErrorKind::AddrInUse);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

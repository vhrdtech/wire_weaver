use std::io;
use std::path::Path;

use interprocess::local_socket::tokio::prelude::*;
use interprocess::local_socket::{ListenerOptions, Name};
use tracing::warn;

pub use interprocess::local_socket::tokio::Stream;

/// Who is on the other side of an accepted or opened connection, checked to be the same user.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Peer {
    /// Process id, where the OS tells it (Linux, Windows; macOS doesn't)
    pub pid: Option<u32>,
}

/// Listens on `path` (a socket file in [runtime_dir](crate::runtime_dir), e.g. `hub.sock`; on Windows the named pipe
/// derived from it). Only the current user can connect: the socket file is 0600 where the OS allows to set it
/// (Linux; elsewhere the 0700 directory guards it), the pipe's DACL gives access to its owner only and denies network
/// logons. [Listener::accept] also checks the peer's user.
///
/// `replace`: remove a socket file left behind by a process that is gone. The caller must know it is gone (the hub
/// holds `hub.lock` for that): a live listener's file would be taken away from it.
pub fn listen(path: &Path, replace: bool) -> io::Result<Listener> {
    let name = name_for(path)?;
    let opts = ListenerOptions::new()
        .name(name.clone())
        .try_overwrite(replace);
    let inner = match create(opts) {
        Err(e) if e.kind() == io::ErrorKind::Unsupported => {
            // no socket file mode on this OS (macOS): the 0700 directory and the peer check keep others out
            ListenerOptions::new()
                .name(name)
                .try_overwrite(replace)
                .create_tokio()?
        }
        r => r?,
    };
    Ok(Listener { inner })
}

#[cfg(unix)]
fn create(opts: ListenerOptions<'_>) -> io::Result<interprocess::local_socket::tokio::Listener> {
    use interprocess::os::unix::local_socket::ListenerOptionsExt;
    opts.mode(0o600).create_tokio()
}

#[cfg(windows)]
fn create(opts: ListenerOptions<'_>) -> io::Result<interprocess::local_socket::tokio::Listener> {
    use interprocess::os::windows::local_socket::ListenerOptionsExt;
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;
    // Protected DACL: deny network logons (NU), full access for the owner (OW, the user that creates the pipe),
    // nobody else. The default DACL would give Everyone read access.
    let sddl = widestring::U16CString::from_str("D:P(D;;GA;;;NU)(A;;GA;;;OW)")
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
    let sd = SecurityDescriptor::deserialize(&sddl)?;
    opts.security_descriptor(sd).create_tokio()
}

/// Opens a connection to a [listen]ing process, refusing one that runs as another user (a name squatted by
/// someone else).
pub async fn connect(path: &Path) -> io::Result<(Stream, Peer)> {
    let stream = Stream::connect(name_for(path)?).await?;
    let peer = check_peer(&stream)?;
    Ok((stream, peer))
}

pub struct Listener {
    inner: interprocess::local_socket::tokio::Listener,
}

impl Listener {
    /// Next connection of the same user. Connections of other users (only possible if the OS let them through the
    /// permissions) are dropped with a warning and not returned.
    pub async fn accept(&self) -> io::Result<(Stream, Peer)> {
        loop {
            let stream = self.inner.accept().await?;
            match check_peer(&stream) {
                Ok(peer) => return Ok((stream, peer)),
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

#[cfg(unix)]
fn check_peer(stream: &Stream) -> io::Result<Peer> {
    let creds = stream.peer_creds()?;
    // SAFETY: geteuid has no preconditions and cannot fail
    let me = unsafe { libc::geteuid() };
    match creds.euid() {
        Some(uid) if uid == me => Ok(Peer {
            pid: creds.pid().map(|p| p as u32),
        }),
        Some(uid) => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("the other side runs as uid {uid}, not {me}"),
        )),
        None => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the OS doesn't tell who is on the other side",
        )),
    }
}

#[cfg(windows)]
fn check_peer(stream: &Stream) -> io::Result<Peer> {
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

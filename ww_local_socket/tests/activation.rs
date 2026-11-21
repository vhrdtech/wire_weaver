//! Socket activation end to end: this test binary starts itself as the child, with a listening socket at
//! descriptor 3 and `LISTEN_FDS` as a service manager sets them.
#![cfg(unix)]

use std::os::fd::AsRawFd;
use std::os::unix::net::UnixListener;
use std::os::unix::process::CommandExt;
use std::process::Command;

use futures_util::io::{AsyncReadExt, AsyncWriteExt};
use ww_local_socket::{ActivationError, PeerPolicy, connect, inherited};

/// Set in the child's environment: the child test runs only there.
const CHILD: &str = "WW_ACTIVATION_CHILD";

#[tokio::test]
async fn a_passed_socket_is_served() {
    let dir = std::env::temp_dir().join(format!("ww-activation-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("api.sock");
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();
    // as a unit's SocketMode=0600 makes it: a looser mode is refused by the same-user policy
    std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
    let fd = listener.as_raw_fd();
    let mut child = Command::new(std::env::current_exe().unwrap());
    child
        .args(["--exact", "child_serves_the_passed_socket", "--nocapture"])
        .env(CHILD, "1")
        .env("LISTEN_FDS", "1")
        .env("LISTEN_FDNAMES", "ww");
    // SAFETY: dup2 and fcntl are async-signal-safe; the closure touches no memory of the parent.
    unsafe {
        child.pre_exec(move || {
            if libc::dup2(fd, 3) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = child.spawn().unwrap();
    drop(listener);

    let (mut stream, peer) = connect(&path).await.unwrap();
    assert_eq!(
        peer.uid,
        std::fs::metadata(&dir)
            .map(|m| std::os::unix::fs::MetadataExt::uid(&m))
            .unwrap()
    );
    stream.write_all(b"ping").await.unwrap();
    let mut answer = [0u8; 4];
    stream.read_exact(&mut answer).await.unwrap();
    assert_eq!(&answer, b"ping");
    assert!(child.wait().unwrap().success(), "the child failed");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The child: takes the socket at descriptor 3, echoes one connection.
#[tokio::test]
async fn child_serves_the_passed_socket() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    // SAFETY: the variable is set before anything else of this process reads the environment; the service
    // manager sets it the same way, between fork and exec.
    unsafe { std::env::set_var("LISTEN_PID", std::process::id().to_string()) };
    assert!(matches!(
        inherited(Some("web"), PeerPolicy::SameUser),
        Err(ActivationError::NoSuchName(_))
    ));
    let listener = inherited(Some("ww"), PeerPolicy::SameUser).unwrap();
    assert!(matches!(
        inherited(Some("ww"), PeerPolicy::SameUser),
        Err(ActivationError::Taken)
    ));
    let (mut stream, peer) = listener.accept().await.unwrap();
    assert_eq!(peer.pid.map(|pid| pid != std::process::id()), Some(true));
    let mut ping = [0u8; 4];
    stream.read_exact(&mut ping).await.unwrap();
    stream.write_all(&ping).await.unwrap();
}

#[test]
fn a_process_started_without_a_socket_is_not_activated() {
    if std::env::var_os(CHILD).is_some() {
        return;
    }
    assert!(matches!(
        inherited(None, PeerPolicy::SameUser),
        Err(ActivationError::NotActivated)
    ));
}

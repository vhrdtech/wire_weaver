use std::path::{Path, PathBuf};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use ww_local_socket::{Answer, Preamble, Status, Target};

use super::*;

fn runtime_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ww-hub-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn config(dir: &Path, idle_ms: u64) -> HubConfig {
    HubConfig {
        runtime_dir: dir.to_path_buf(),
        idle_exit: Duration::from_millis(idle_ms),
        linger: Duration::from_secs(30),
        version: HUB_VERSION.into(),
        sync: true,
        sync_linger: Some(Duration::from_secs(30 * 60)),
        sync_options: wire_weaver_client::clock_sync::Options::default(),
        state_dir: None,
    }
}

/// Starts a hub, returns its task and a shutdown handle once it listens.
async fn start(dir: &Path, idle_ms: u64) -> (tokio::task::JoinHandle<Exit>, Shutdown) {
    let bound = Bound::bind(config(dir, idle_ms))
        .unwrap()
        .expect("no other hub");
    let shutdown = bound.shutdown_handle();
    (tokio::spawn(bound.run()), shutdown)
}

async fn hello(dir: &Path, target: Target) -> (Answer, ww_local_socket::Stream) {
    let (stream, _) = ww_local_socket::connect(&dir.join(HUB_SOCKET))
        .await
        .unwrap();
    let (mut rx, mut tx) = stream.split();
    tx.write_all(&Preamble::new(target).encode()).await.unwrap();
    let answer = Answer::read(&mut rx).await.unwrap();
    (
        answer,
        interprocess::local_socket::tokio::Stream::reunite(rx, tx).unwrap(),
    )
}

#[tokio::test]
async fn one_hub_per_user() {
    let dir = runtime_dir("one");
    let (first, shutdown) = start(&dir, 60_000).await;
    assert_eq!(
        serve(config(&dir, 60_000)).await.unwrap(),
        Exit::AlreadyRunning
    );
    shutdown.shutdown();
    assert_eq!(first.await.unwrap(), Exit::Shutdown);
    // the lock is free again
    let (second, shutdown) = start(&dir, 60_000).await;
    shutdown.shutdown();
    assert_eq!(second.await.unwrap(), Exit::Shutdown);
}

#[tokio::test]
async fn hub_answers_preambles() {
    let dir = runtime_dir("answers");
    let (hub, shutdown) = start(&dir, 60_000).await;
    let (answer, _conn) = hello(&dir, Target::Hub).await;
    assert_eq!(answer.status, Status::Ok);
    assert_eq!(answer.hub_version, HUB_VERSION);
    let (answer, _) = hello(&dir, Target::Device("usb:c0de:cafe:1".into())).await;
    assert_eq!(answer.status, Status::UnknownDevice);
    shutdown.shutdown();
    assert_eq!(hub.await.unwrap(), Exit::Shutdown);
}

#[tokio::test]
async fn exits_when_idle_not_while_a_client_is_connected() {
    let dir = runtime_dir("idle");
    let (hub, _shutdown) = start(&dir, 300).await;
    let (answer, conn) = hello(&dir, Target::Hub).await;
    assert_eq!(answer.status, Status::Ok);
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(!hub.is_finished(), "a connected client keeps the hub up");
    let left = Instant::now();
    drop(conn);
    assert_eq!(hub.await.unwrap(), Exit::Idle);
    let waited = left.elapsed();
    assert!(waited >= Duration::from_millis(300), "{waited:?}");
    #[cfg(unix)]
    assert!(!dir.join(HUB_SOCKET).exists(), "socket file removed");
}

#[cfg(unix)]
#[tokio::test]
async fn stale_socket_file_replaced() {
    let dir = runtime_dir("stale");
    std::fs::write(dir.join(HUB_SOCKET), b"left over").unwrap();
    let (hub, shutdown) = start(&dir, 60_000).await;
    let (answer, _conn) = hello(&dir, Target::Hub).await;
    assert_eq!(answer.status, Status::Ok);
    shutdown.shutdown();
    hub.await.unwrap();
}

#[tokio::test]
async fn silent_or_bad_clients_are_dropped() {
    let dir = runtime_dir("bad");
    let (hub, shutdown) = start(&dir, 60_000).await;
    let (stream, _) = ww_local_socket::connect(&dir.join(HUB_SOCKET))
        .await
        .unwrap();
    let (mut rx, mut tx) = stream.split();
    tx.write_all(b"GET / HTTP/1.1\r\n\r\n").await.unwrap();
    let mut b = [0u8; 1];
    // closed without an answer (a reset, as the rest of the request is unread)
    match rx.read(&mut b).await {
        Ok(n) => assert_eq!(n, 0),
        Err(e) => assert_eq!(e.kind(), io::ErrorKind::ConnectionReset),
    }
    let (stream, _) = ww_local_socket::connect(&dir.join(HUB_SOCKET))
        .await
        .unwrap();
    let (mut rx, mut tx) = stream.split();
    tx.write_all(b"WWHB\x09").await.unwrap();
    tx.write_all(&[0; 3]).await.unwrap();
    let answer = Answer::read(&mut rx).await.unwrap();
    assert_eq!(answer.status, Status::PreambleVersion);
    shutdown.shutdown();
    hub.await.unwrap();
}

mod api {
    use super::*;
    use crate::client::HubClient;

    async fn connect(dir: &Path) -> HubClient {
        let path = dir.join(HUB_SOCKET).display().to_string();
        HubClient::config(|c| c.hub_api_at(path.clone()))
            .connect()
            .await
            .expect("connect to the hub API")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn info_clients_keep_shutdown() {
        let dir = runtime_dir("api");
        let (hub, _shutdown) = start(&dir, 300).await;
        let mut client = connect(&dir).await;

        let info = client.info().call().await.unwrap();
        assert_eq!(info.hub_version, HUB_VERSION);
        assert_eq!(info.pid, std::process::id());
        assert!(info.socket.ends_with(HUB_SOCKET), "{}", info.socket);

        let clients = client.registry().clients().call().await.unwrap();
        assert_eq!(clients.len(), 1, "{clients:?}");
        assert_eq!(clients[0].pid, std::process::id());
        assert!(
            !clients[0].program.is_empty(),
            "client metadata from link setup: {clients:?}"
        );

        // whatever is plugged into this PC: the call answers, every entry has a key
        let devices = client.registry().devices().call().await.unwrap();
        assert!(devices.iter().all(|d| !d.key.is_empty()), "{devices:?}");

        // keep holds the hub up after the client leaves, past the idle timeout
        client.keep(1).call().await.unwrap();
        client.disconnect().asynch().await.unwrap();
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert!(!hub.is_finished(), "kept");

        let client = connect(&dir).await;
        client.shutdown().call().await.unwrap();
        assert_eq!(hub.await.unwrap(), Exit::Shutdown);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn events_name_joining_clients() {
        let dir = runtime_dir("events");
        let (hub, shutdown) = start(&dir, 60_000).await;
        let watcher = connect(&dir).await;
        let mut events = watcher.registry().events().call().await.unwrap();
        let other = connect(&dir).await;
        let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
            .await
            .expect("an event")
            .unwrap();
        assert!(
            matches!(event, ww_hub_api::EventOwned::ClientJoined { .. }),
            "{event:?}"
        );
        other.disconnect().asynch().await.unwrap();
        let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
            .await
            .expect("an event")
            .unwrap();
        assert!(
            matches!(event, ww_hub_api::EventOwned::ClientLeft { .. }),
            "{event:?}"
        );
        shutdown.shutdown();
        hub.await.unwrap();
    }
}

//! The hub's version window (SRV-30): a client says its `ww_link` and `ww_client_server` versions in the preamble, a
//! hub that doesn't serve them refuses with status 4, and the client's error names both sides and the fix.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hub_reconnect::client::Client;
use interprocess::local_socket::traits::tokio::Stream as _;
use tokio::io::AsyncWriteExt;
use wire_weaver_client::Error;
use ww_hub::{Bound, HubConfig};
use ww_local_socket::{Answer, Preamble, Status, Target, Version};

fn runtime_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ww-hub-versions-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn hello(socket: &Path, preamble: &Preamble) -> Answer {
    let (stream, _) = ww_local_socket::connect(socket).await.unwrap();
    let (mut rx, mut tx) = stream.split();
    tx.write_all(&preamble.encode()).await.unwrap();
    Answer::read(&mut rx).await.unwrap()
}

/// A client built with a `ww_link` the hub dropped (an older major) is refused before anything else; one that
/// doesn't say its versions (preamble version 1) is served as before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn old_client_vs_hub_that_dropped_its_version() {
    let dir = runtime_dir("dropped");
    let socket = dir.join(ww_local_socket::files::HUB_SOCKET);
    let bound = Bound::bind(HubConfig {
        runtime_dir: dir.clone(),
        idle_exit: Duration::from_secs(60),
        linger: Duration::from_secs(30),
        version: ww_hub::HUB_VERSION.into(),
        sync: false,
        sync_linger: None,
        sync_options: Default::default(),
        adopt_attach: std::time::Duration::from_secs(5),
        state_dir: None,
    })
    .unwrap()
    .unwrap();
    let shutdown = bound.shutdown_handle();
    let hub = tokio::spawn(bound.run());

    let old =
        Preamble::new(Target::Hub).with_versions(Version::new(0, 0, 1), Version::new(0, 0, 1));
    let answer = hello(&socket, &old).await;
    assert_eq!(answer.status, Status::Versions);
    assert_eq!(answer.hub_version, ww_hub::HUB_VERSION);
    let unknown = Preamble::new(Target::Hub).v1();
    assert_eq!(hello(&socket, &unknown).await.status, Status::Ok);

    shutdown.shutdown();
    hub.await.unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// What a client hears from a hub that doesn't serve it: both versions and what to do, and no reconnect attempts.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn refused_versions_name_both_sides_and_the_fix() {
    let dir = runtime_dir("refused");
    let socket = dir.join(ww_local_socket::files::HUB_SOCKET);
    let listener = ww_local_socket::listen(&socket, true).unwrap();
    // a hub of a later, incompatible release
    let fake = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (mut rx, mut tx) = stream.split();
        let p = Preamble::read(&mut rx).await.unwrap();
        tx.write_all(&Answer::new(Status::Versions, "9.0.0").encode())
            .await
            .unwrap();
        p
    });
    let e =
        match Client::config(|c| c.hub_device_at(socket.display().to_string(), "usb:c0de:cafe:1"))
            .connect()
            .await
        {
            Ok(_) => panic!("connected to a hub that refused"),
            Err(e) => e,
        };
    let seen = fake.await.unwrap();
    assert!(seen.link_version.is_some() && seen.api_model_version.is_some());
    let e = e.downcast::<Error>().expect("a client error");
    assert!(matches!(e, Error::HubRefused { status: 4, .. }), "{e}");
    let text = e.to_string();
    let link = seen.link_version.unwrap().to_string();
    for part in [
        "9.0.0",
        "ww_link",
        link.as_str(),
        "ww_client_server",
        "rebuild",
        "WW_HUB=off",
    ] {
        assert!(text.contains(part), "{part:?} in {text}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A hub that only reads the first preamble layout answers the second with status 3: the client asks again with the
/// first one, without its versions.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hub_reading_only_version_1_is_asked_again() {
    let dir = runtime_dir("v1");
    let socket = dir.join(ww_local_socket::files::HUB_SOCKET);
    let listener = ww_local_socket::listen(&socket, true).unwrap();
    let seen = Arc::new(Mutex::new(vec![]));
    let s = seen.clone();
    tokio::spawn(async move {
        for _ in 0..2 {
            let (stream, _) = listener.accept().await.unwrap();
            let (mut rx, mut tx) = stream.split();
            let p = Preamble::read(&mut rx).await.unwrap();
            s.lock().unwrap().push(p.version);
            let status = match p.version {
                1 => Status::UnknownDevice,
                _ => Status::PreambleVersion,
            };
            tx.write_all(&Answer::new(status, "0.5.0-v1").encode())
                .await
                .unwrap();
        }
    });
    let e =
        match Client::config(|c| c.hub_device_at(socket.display().to_string(), "usb:c0de:cafe:2"))
            .connect()
            .await
        {
            Ok(_) => panic!("the stand-in knows no device"),
            Err(e) => e,
        };
    assert!(
        matches!(
            e.downcast_ref::<Error>(),
            Some(Error::HubRefused { status: 1, .. })
        ),
        "{e}"
    );
    assert_eq!(*seen.lock().unwrap(), [2, 1]);
    let _ = std::fs::remove_dir_all(&dir);
}

//! A newer hub takes over from the running one (SRV-30) while clients are connected: they hear `Event::Handover` and
//! `Disconnect { HubHandover }`, reconnect to the new hub by themselves (CLT-40) and go on; only a strictly newer hub
//! is accepted. Both hubs run in this process, next to the device.

use std::path::{Path, PathBuf};
use std::time::Duration;

use hub_reconnect::client::Client;
use hub_reconnect::start_device;
use tokio::sync::broadcast::Receiver;
use wire_weaver_client::hub::HubClient;
use wire_weaver_client::{ConnectionEvent, GapReason, StreamEvent};
use ww_hub::{Bound, Exit, HubConfig, TakeOverError};
use ww_hub_api::EventOwned;

fn runtime_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ww-hub-handover-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn config(dir: &Path, version: &str) -> HubConfig {
    HubConfig {
        runtime_dir: dir.to_path_buf(),
        idle_exit: Duration::from_secs(60),
        linger: Duration::from_secs(30),
        version: version.into(),
        sync: false,
        sync_linger: None,
        sync_options: Default::default(),
        adopt_attach: std::time::Duration::from_secs(5),
        state_dir: None,
    }
}

async fn event(events: &mut Receiver<ConnectionEvent>) -> ConnectionEvent {
    tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .expect("a connection event in time")
        .expect("events open")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn newer_hub_takes_over_with_clients_connected() {
    let _ = tracing_subscriber::fmt::try_init();
    let (_device, seen) = start_device("hub_reconnect/handover");
    let dir = runtime_dir("takes_over");
    let socket = dir
        .join(ww_local_socket::files::HUB_SOCKET)
        .display()
        .to_string();
    let old = Bound::bind(config(&dir, "0.0.1")).unwrap().unwrap();
    let old = tokio::spawn(old.run());

    let mut client =
        Client::config(|c| c.hub_device_at(socket.clone(), "inproc:hub_reconnect/handover"))
            .connect()
            .await
            .unwrap();
    let mut events = client.cmd.connection_events();
    let mut ticks = client.ticks().call().await.unwrap();
    client.emit(2).call().await.unwrap();
    for want in 0..2 {
        assert_eq!(ticks.recv().await.unwrap(), want);
    }
    // a program watching the registry
    let watcher = HubClient::config(|c| c.hub_api_at(socket.clone()).hub_reconnect(None))
        .connect()
        .await
        .unwrap();
    let mut registry = watcher.registry().events().call().await.unwrap();

    let new = tokio::spawn(Bound::take_over(config(&dir, "9.0.0")));
    let new = new.await.unwrap().unwrap().expect("took over");
    assert_eq!(old.await.unwrap(), Exit::Handover);
    let new_shutdown = new.shutdown_handle();
    let new = tokio::spawn(new.run());

    let mut handover_seen = false;
    while let Ok(Ok(e)) = tokio::time::timeout(Duration::from_secs(1), registry.recv()).await {
        if e == EventOwned::Handover {
            handover_seen = true;
            break;
        }
    }
    assert!(handover_seen, "the registry's listeners hear the handover");

    match event(&mut events).await {
        ConnectionEvent::Disconnected {
            reason,
            reconnecting,
        } => assert_eq!((reason, reconnecting), (GapReason::HubHandover, true)),
        other => panic!("expected Disconnected, got {other:?}"),
    }
    loop {
        match event(&mut events).await {
            ConnectionEvent::Reconnected { resumed, .. } => {
                assert_eq!(resumed, 1);
                break;
            }
            ConnectionEvent::AttemptFailed { .. } => {}
            other => panic!("expected Reconnected, got {other:?}"),
        }
    }
    assert_eq!(client.add(20, 22).call().await.unwrap(), 42);
    client.emit(1).call().await.unwrap();
    loop {
        let e = tokio::time::timeout(Duration::from_secs(2), ticks.recv_event())
            .await
            .expect("an event in time")
            .expect("stream open");
        if let StreamEvent::Item { value, .. } = e {
            assert_eq!(value, 2);
            break;
        }
    }
    // the new hub serves on the same socket
    let info = HubClient::config(|c| c.hub_api_at(socket.clone()))
        .connect()
        .await
        .unwrap()
        .info()
        .call()
        .await
        .unwrap();
    assert_eq!(info.hub_version, "9.0.0");
    assert_eq!(seen.lock().unwrap().adds, [(20, 22)]);

    new_shutdown.shutdown();
    new.await.unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// A hub that isn't strictly newer is refused and the running one stays; the would-be successor's socket goes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_a_strictly_newer_hub_takes_over() {
    let _ = tracing_subscriber::fmt::try_init();
    let dir = runtime_dir("not_newer");
    let socket = dir
        .join(ww_local_socket::files::HUB_SOCKET)
        .display()
        .to_string();
    let running = Bound::bind(config(&dir, "0.5.0")).unwrap().unwrap();
    let shutdown = running.shutdown_handle();
    let running = tokio::spawn(running.run());
    for version in ["0.5.0", "0.4.9"] {
        match Bound::take_over(config(&dir, version)).await.unwrap() {
            Err(TakeOverError::NotNewer { running }) => assert_eq!(running, "0.5.0"),
            Err(e) => panic!("{version}: expected NotNewer, got {e}"),
            Ok(_) => panic!("{version} took over from 0.5.0"),
        }
    }
    let info = HubClient::config(|c| c.hub_api_at(socket.clone()))
        .connect()
        .await
        .unwrap()
        .info()
        .call()
        .await
        .unwrap();
    assert_eq!(info.hub_version, "0.5.0");
    let left: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.ends_with(".sock") && n != ww_local_socket::files::HUB_SOCKET)
        .collect();
    assert!(left.is_empty(), "{left:?}");
    shutdown.shutdown();
    running.await.unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

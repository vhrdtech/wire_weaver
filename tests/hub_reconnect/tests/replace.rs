//! A program whose own hub is newer than the running one replaces it (SRV-30) while a client is connected: the old
//! hub (here, in this process, an older version) hands over to the new one, which `hub_dev` starts from itself, and
//! the client goes on through it (CLT-40). One test in this binary, as it sets the environment the hub is found by.

use std::process::Command;
use std::time::Duration;

use hub_reconnect::client::Client;
use wire_weaver_client::{ConnectionEvent, GapReason, StreamEvent};
use ww_hub::{Bound, Exit, HubConfig};

const HUB_DEV: &str = env!("CARGO_BIN_EXE_hub_dev");

fn hub_dev(arg: &str) -> String {
    let out = Command::new(HUB_DEV).arg(arg).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_newer_program_replaces_the_running_hub() {
    let _ = tracing_subscriber::fmt::try_init();
    let dir = std::env::temp_dir().join(format!("ww-hub-replace-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // SAFETY: the only test of this binary, set before anything reads them
    unsafe {
        std::env::set_var(ww_local_socket::RUNTIME_DIR_ENV, &dir);
        std::env::set_var(ww_hub::IDLE_EXIT_ENV, "5");
    }
    let (_device, _seen) = hub_reconnect::start_device_ipc(&dir.join("dev.sock"));
    let key = format!("ipc:{}", dir.join("dev.sock").display());
    let old = Bound::bind(HubConfig {
        runtime_dir: ww_local_socket::runtime_dir().unwrap(),
        idle_exit: Duration::from_secs(60),
        linger: Duration::from_secs(30),
        version: "0.0.1".into(),
        sync: false,
        sync_linger: None,
        sync_options: Default::default(),
        adopt_attach: std::time::Duration::from_secs(5),
        state_dir: None,
    })
    .unwrap()
    .unwrap();
    let old = tokio::spawn(old.run());

    let mut client = Client::config(|c| c.hub_device(key.clone()))
        .connect()
        .await
        .unwrap();
    let mut events = client.cmd.connection_events();
    let mut ticks = client.ticks().call().await.unwrap();
    client.emit(1).call().await.unwrap();
    assert_eq!(ticks.recv().await.unwrap(), 0);

    // hub_dev can host a newer hub (its own): asking for the hub replaces the old one
    let answer = hub_dev("ensure");
    assert!(answer.starts_with(ww_hub::HUB_VERSION), "{answer}");
    assert_eq!(old.await.unwrap(), Exit::Handover);

    loop {
        match tokio::time::timeout(Duration::from_secs(5), events.recv()).await {
            Ok(Ok(ConnectionEvent::Disconnected { reason, .. })) => {
                assert_eq!(reason, GapReason::HubHandover)
            }
            Ok(Ok(ConnectionEvent::Reconnected { resumed, .. })) => {
                assert_eq!(resumed, 1);
                break;
            }
            Ok(Ok(ConnectionEvent::AttemptFailed { .. })) => {}
            other => panic!("expected a reconnect, got {other:?}"),
        }
    }
    client.emit(1).call().await.unwrap();
    loop {
        let e = tokio::time::timeout(Duration::from_secs(2), ticks.recv_event())
            .await
            .expect("an event in time")
            .expect("stream open");
        if let StreamEvent::Item { value, .. } = e {
            assert_eq!(value, 1);
            break;
        }
    }
    // the same hub answers again: nothing newer to replace it with
    assert_eq!(hub_dev("ensure"), answer);
    hub_dev("stop");
    drop(client);
    let _ = std::fs::remove_dir_all(&dir);
}

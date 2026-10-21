//! A client of a device behind the ww hub reconnects by itself when the hub goes and a new one comes (CLT-40): the
//! hub is shut down and bound again in this process, next to the device.

use std::path::{Path, PathBuf};
use std::time::Duration;

use hub_reconnect::client::Client;
use hub_reconnect::start_device;
use tokio::sync::broadcast::Receiver;
use wire_weaver_client::{ConnectionEvent, GapReason, StreamEvent};
use ww_hub::{Bound, Exit, HubConfig, Shutdown};

fn runtime_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ww-hub-reconnect-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn bind(dir: &Path) -> (tokio::task::JoinHandle<Exit>, Shutdown) {
    let bound = Bound::bind(HubConfig {
        runtime_dir: dir.to_path_buf(),
        idle_exit: Duration::from_secs(60),
        linger: Duration::from_secs(30),
        version: ww_hub::HUB_VERSION.into(),
        sync: false,
        sync_linger: None,
        sync_options: Default::default(),
        state_dir: None,
    })
    .unwrap()
    .expect("no other hub in this dir");
    let shutdown = bound.shutdown_handle();
    (tokio::spawn(bound.run()), shutdown)
}

async fn event(events: &mut Receiver<ConnectionEvent>) -> ConnectionEvent {
    tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .expect("a connection event in time")
        .expect("events open")
}

/// Waits for `Reconnected`, skipping failed attempts.
async fn reconnected(events: &mut Receiver<ConnectionEvent>) -> usize {
    loop {
        match event(events).await {
            ConnectionEvent::Reconnected { resumed, .. } => return resumed,
            ConnectionEvent::AttemptFailed { .. } => {}
            other => panic!("expected Reconnected, got {other:?}"),
        }
    }
}

/// The hub goes (shut down) with a counted stream open: the client sees `Disconnected`, a call in the gap fails at
/// once, and once a new hub listens the client is back by itself, the stream resumed: the ticks the device sends then
/// arrive on the same handle.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hub_restart_reconnects_and_resumes_the_stream() {
    let _ = tracing_subscriber::fmt::try_init();
    let (_device, seen) = start_device("hub_reconnect/restart");
    let dir = runtime_dir("restart");
    let socket = dir.join(ww_local_socket::files::HUB_SOCKET);
    let (hub, shutdown) = bind(&dir);

    let mut client = Client::config(|c| {
        c.hub_device_at(socket.display().to_string(), "inproc:hub_reconnect/restart")
    })
    .connect()
    .await
    .expect("connect through the hub");
    let mut events = client.cmd.connection_events();
    let mut ticks = client.ticks().call().await.unwrap();
    client.emit(3).call().await.unwrap();
    for want in 0..3 {
        assert_eq!(ticks.recv().await.unwrap(), want);
    }

    shutdown.shutdown();
    assert_eq!(hub.await.unwrap(), Exit::Shutdown);
    match event(&mut events).await {
        ConnectionEvent::Disconnected {
            reason,
            reconnecting,
        } => {
            assert!(reconnecting, "{reason}");
            assert!(
                !matches!(reason, GapReason::HubHandover | GapReason::Lent),
                "{reason}"
            );
        }
        other => panic!("expected Disconnected, got {other:?}"),
    }
    let t = std::time::Instant::now();
    let e = client.add(1, 2).call().await.unwrap_err();
    assert!(format!("{e:?}").contains("Disconnected"), "{e:?}");
    assert!(
        t.elapsed() < Duration::from_millis(500),
        "{:?}",
        t.elapsed()
    );

    let (hub, shutdown) = bind(&dir);
    assert_eq!(reconnected(&mut events).await, 1, "the counted stream");
    assert_eq!(client.add(2, 3).call().await.unwrap(), 5);
    client.emit(2).call().await.unwrap();
    let mut got = vec![];
    while got.len() < 2 {
        let e = tokio::time::timeout(Duration::from_secs(2), ticks.recv_event())
            .await
            .expect("an event in time")
            .expect("stream open");
        match e {
            StreamEvent::Item { value, .. } => got.push(value),
            StreamEvent::Resumed { .. } | StreamEvent::Disconnected | StreamEvent::Gap { .. } => {}
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(got, [3, 4]);
    // the call in the gap never reached the device, nothing was sent twice
    assert_eq!(seen.lock().unwrap().adds, [(2, 3)]);

    shutdown.shutdown();
    hub.await.unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// While the hub answers that the device is lent (preamble status 5, SRV-32), the client keeps waiting past its
/// give-up time, without starting anything, and is back once the device is.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lent_device_waits_past_the_give_up_time() {
    use interprocess::local_socket::traits::tokio::Stream as _;
    use tokio::io::AsyncWriteExt;
    use ww_local_socket::{Answer, Preamble, Status};

    let _ = tracing_subscriber::fmt::try_init();
    let (_device, _seen) = start_device("hub_reconnect/lent");
    let dir = runtime_dir("lent");
    let socket = dir.join(ww_local_socket::files::HUB_SOCKET);
    let (hub, shutdown) = bind(&dir);
    let give_up = Duration::from_millis(100);
    let mut client = Client::config(|c| {
        c.hub_device_at(socket.display().to_string(), "inproc:hub_reconnect/lent")
            .hub_reconnect(Some(give_up))
    })
    .connect()
    .await
    .unwrap();
    let mut events = client.cmd.connection_events();
    shutdown.shutdown();
    hub.await.unwrap();
    assert!(matches!(
        event(&mut events).await,
        ConnectionEvent::Disconnected { .. }
    ));

    // a stand-in for a hub that lent the device: every connection is answered "lent"
    let listener = ww_local_socket::listen(&socket, true).unwrap();
    let lender = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let (mut rx, mut tx) = stream.split();
            if Preamble::read(&mut rx).await.is_ok() {
                _ = tx
                    .write_all(&Answer::new(Status::from_u8(5), "0.0.0-lent").encode())
                    .await;
            }
        }
    });
    let lent_for = give_up * 3;
    let lent_until = std::time::Instant::now() + lent_for;
    let mut failed = 0;
    while std::time::Instant::now() < lent_until {
        match event(&mut events).await {
            ConnectionEvent::AttemptFailed { .. } => failed += 1,
            other => panic!("expected attempts while lent, got {other:?}"),
        }
    }
    assert!(failed > 2, "{failed} attempts");
    lender.abort();
    _ = lender.await;
    let (hub, shutdown) = bind(&dir);
    reconnected(&mut events).await;
    assert_eq!(client.add(1, 1).call().await.unwrap(), 2);
    shutdown.shutdown();
    hub.await.unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// No hub comes back within the give-up time: the client gives up, the waiting stream ends and calls fail.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_hub_within_the_give_up_time_ends_the_streams() {
    let _ = tracing_subscriber::fmt::try_init();
    let (_device, _seen) = start_device("hub_reconnect/give_up");
    let dir = runtime_dir("give_up");
    let socket = dir.join(ww_local_socket::files::HUB_SOCKET);
    let (hub, shutdown) = bind(&dir);
    let mut client = Client::config(|c| {
        c.hub_device_at(socket.display().to_string(), "inproc:hub_reconnect/give_up")
            .hub_reconnect(Some(Duration::from_millis(200)))
    })
    .connect()
    .await
    .unwrap();
    let mut events = client.cmd.connection_events();
    let mut ticks = client.ticks().call().await.unwrap();
    shutdown.shutdown();
    hub.await.unwrap();
    loop {
        match event(&mut events).await {
            ConnectionEvent::GaveUp { .. } => break,
            ConnectionEvent::Disconnected { .. } | ConnectionEvent::AttemptFailed { .. } => {}
            other => panic!("unexpected {other:?}"),
        }
    }
    let end = tokio::time::timeout(Duration::from_secs(1), ticks.recv()).await;
    assert!(matches!(end, Ok(Err(_))), "the stream ends: {end:?}");
    assert!(client.add(1, 1).call().await.is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

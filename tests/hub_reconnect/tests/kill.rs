//! The hub killed (`SIGKILL` / `TerminateProcess`) under load, twice: the client comes back by itself through a new
//! hub it starts (CLT-40), calls in flight fail with `Disconnected` and never hang, none is sent twice, and the
//! stream goes on. The hub is `hub_dev`, a separate process hosting the device; one test in this binary, as it sets
//! the environment the client finds the hub by.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use hub_reconnect::client::Client;
use wire_weaver_client::hub::HubClient;
use wire_weaver_client::{ConnectionEvent, StreamEvent};

const HUB_DEV: &str = env!("CARGO_BIN_EXE_hub_dev");

fn kill(pid: u32) {
    #[cfg(unix)]
    let status = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status();
    #[cfg(windows)]
    let status = std::process::Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .status();
    assert!(status.unwrap().success(), "killing the hub {pid}");
}

async fn hub_pid() -> u32 {
    HubClient::config(|c| c.hub_api().hub_reconnect(None))
        .connect()
        .await
        .unwrap()
        .info()
        .call()
        .await
        .unwrap()
        .pid
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hub_killed_under_load_clients_come_back() {
    let _ = tracing_subscriber::fmt::try_init();
    let dir = std::env::temp_dir().join(format!("ww-hub-kill-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // SAFETY: the only test of this binary, set before anything reads them
    unsafe {
        std::env::set_var(ww_local_socket::RUNTIME_DIR_ENV, &dir);
        std::env::set_var(wire_weaver_client::hub::EXE_ENV, HUB_DEV);
        // a hub a failed test leaves behind goes soon
        std::env::set_var(ww_hub::IDLE_EXIT_ENV, "5");
    }

    // the device outlives the hub: served by this process, opened by the hub as `ipc:<socket>`
    let (_device, seen) = hub_reconnect::start_device_ipc(&dir.join("dev.sock"));
    let key = format!("ipc:{}", dir.join("dev.sock").display());
    // the user's hub, started by the client
    let mut client = Client::config(|c| c.hub_device(key.clone()))
        .connect()
        .await
        .expect("connect, starting the hub");
    let mut events = client.cmd.connection_events();
    let mut ticks = client.ticks().call().await.unwrap();

    // load: calls in flight all the time, each answered right or failed, never hanging
    let stop = Arc::new(AtomicBool::new(false));
    let (ok, failed) = (Arc::new(AtomicU32::new(0)), Arc::new(AtomicU32::new(0)));
    let load = {
        let (stop, ok, failed) = (stop.clone(), ok.clone(), failed.clone());
        // a clone of the same connection
        let mut client = Client {
            cmd: client.cmd.clone(),
        };
        tokio::spawn(async move {
            let mut i = 0u32;
            while !stop.load(Ordering::Relaxed) {
                let mut pending = vec![];
                for _ in 0..20 {
                    i += 1;
                    match client.add(i, 1).send().await {
                        Ok(p) => pending.push((i, p)),
                        Err(_) => _ = failed.fetch_add(1, Ordering::Relaxed),
                    }
                }
                for (i, p) in pending {
                    let t = Instant::now();
                    match p.reply().await {
                        Ok(v) => {
                            assert_eq!(v, i + 1);
                            ok.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(e) => {
                            assert!(format!("{e:?}").contains("Disconnected"), "{e:?}");
                            failed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    assert!(t.elapsed() < Duration::from_secs(2), "a call hung");
                }
                tokio::task::yield_now().await;
            }
        })
    };

    let mut next_tick = 0u32;
    for round in 0..2 {
        while ok.load(Ordering::Relaxed) < 50 * (round + 1) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let before = hub_pid().await;
        kill(before);
        let t = Instant::now();
        loop {
            match tokio::time::timeout(Duration::from_secs(5), events.recv()).await {
                Ok(Ok(ConnectionEvent::Reconnected { resumed, .. })) => {
                    assert_eq!(resumed, 1, "the counted stream");
                    break;
                }
                Ok(Ok(ConnectionEvent::GaveUp { error })) => panic!("gave up: {error}"),
                Ok(Ok(_)) => {}
                other => panic!("no reconnect within 5 s: {other:?}"),
            }
        }
        let after = hub_pid().await;
        assert_ne!(before, after, "a new hub");
        eprintln!(
            "round {round}: back after {:?} through hub {after}",
            t.elapsed()
        );
        // the stream goes on where it was
        client.emit(2).call().await.unwrap();
        let mut got = vec![];
        while got.len() < 2 {
            let e = tokio::time::timeout(Duration::from_secs(2), ticks.recv_event())
                .await
                .expect("an event in time")
                .expect("stream open");
            if let StreamEvent::Item { value, .. } = e {
                got.push(value);
            }
        }
        assert_eq!(got, [next_tick, next_tick + 1], "round {round}");
        next_tick += 2;
    }
    stop.store(true, Ordering::Relaxed);
    tokio::time::timeout(Duration::from_secs(5), load)
        .await
        .expect("the load ends")
        .unwrap();
    assert!(ok.load(Ordering::Relaxed) >= 100);
    // every call the device saw, once: none was sent again
    let adds = seen.lock().unwrap().adds.clone();
    let mut sorted = adds.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), adds.len(), "a call reached the device twice");
    assert!(adds.len() as u32 >= ok.load(Ordering::Relaxed));
    eprintln!(
        "{} calls answered, {} failed with Disconnected",
        ok.load(Ordering::Relaxed),
        failed.load(Ordering::Relaxed)
    );

    HubClient::config(|c| c.hub_api().hub_reconnect(None))
        .connect()
        .await
        .unwrap()
        .shutdown()
        .call()
        .await
        .unwrap();
    drop(client);
    let _ = std::fs::remove_dir_all(&dir);
}

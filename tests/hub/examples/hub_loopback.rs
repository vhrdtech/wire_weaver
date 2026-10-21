//! `ww usb-loopback`'s test, direct and through the ww hub (SRV-29), to see what the hub costs.
//!
//! ```sh
//! cargo run --release -p hub --example hub_loopback                 # an in-process device
//! cargo run --release -p hub --example hub_loopback -- usb:c0de:cafe:<serial> [seconds] [packet size] [rounds]
//! ```
//!
//! Direct and hub runs alternate for `rounds` (3) each, the medians are compared: a busy PC moves single runs by
//! more than the hub costs.
//!
//! The hub runs in this process, on a socket in a temporary runtime dir, so it doesn't meet the user's own hub.

use std::time::Duration;

use tokio::sync::mpsc;
use wire_weaver::prelude::*;
use wire_weaver::ww_version::Version;
use wire_weaver_client::internal::{Command, LoopbackParams, TestProgress};
use wire_weaver_client::{ClientConfig, DynClient};

/// A device that has no API, only the link: `ww_device` answers loopback messages itself.
struct NoApi;

impl WireWeaverAsyncApiBackend for NoApi {
    type Medium = ();

    async fn process_bytes<'a>(
        &mut self,
        _out: &mut EventWriter<'_, impl MessageSink>,
        _medium: (),
        _data: &[u8],
        _scratch: &'a mut [u8],
    ) -> Result<&'a [u8], ShrinkWrapError> {
        Ok(&[])
    }

    fn version(&self) -> FullVersion<'_> {
        FullVersion::new("loopback", Version::new(0, 1, 0))
    }
}

fn start_in_process_device(path: &str) {
    let (tx, rx) = wire_weaver_client::in_process::device(path, 1024);
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let mut scratch = vec![0u8; 1024];
                let mut event_scratch = vec![0u8; 1024];
                let config = ww_device::LinkConfig::new(
                    FullVersion::new("loopback", Version::new(0, 1, 0)),
                    wire_weaver::ww_version::ApiHashPair::empty(),
                    ww_client_server::COMPACT_VERSION,
                );
                let mut server = ww_device::Server::new(
                    config,
                    tx,
                    rx,
                    wire_weaver_client::in_process::TokioClock::new(),
                    &mut scratch,
                    &mut event_scratch,
                );
                server.run(&mut NoApi).await
            })
    });
}

/// Waits until the hub closed the device (its linger, 1 s here), so the next direct run can open it.
async fn hub_closed() {
    tokio::time::sleep(Duration::from_millis(1200)).await;
}

/// Each figure's median over the runs.
fn median(mut runs: Vec<Run>) -> Run {
    let mid = |v: &mut Vec<f32>| {
        v.sort_by(f32::total_cmp);
        v[v.len() / 2]
    };
    let pick = |f: fn(&Run) -> f32, runs: &Vec<Run>| mid(&mut runs.iter().map(f).collect());
    let r = Run {
        round_trips: pick(|r| r.round_trips, &runs),
        tx: (pick(|r| r.tx.0, &runs), pick(|r| r.tx.1, &runs)),
        rx: (pick(|r| r.rx.0, &runs), pick(|r| r.rx.1, &runs)),
        errors: runs.iter().map(|r| r.errors).sum(),
    };
    runs.clear();
    r
}

/// Round trips per second, then messages and bytes per second each way.
#[derive(Default, Debug)]
struct Run {
    round_trips: f32,
    tx: (f32, f32),
    rx: (f32, f32),
    errors: u64,
}

async fn run(config: ClientConfig, seconds: u64, packet_size: Option<usize>) -> Run {
    let device = DynClient::from_config(config.traits_only())
        .connect()
        .await
        .expect("connect");
    eprintln!(
        "accumulation window {:?}",
        device.device_api_info().accumulation_time
    );
    let (progress_tx, mut progress) = mpsc::unbounded_channel();
    device
        .cmd()
        .send(Command::LoopbackTest {
            test_duration: Duration::from_secs(seconds),
            packet_size,
            params: Box::new(LoopbackParams::default()),
            progress_tx,
        })
        .await
        .unwrap();
    let mut r = Run::default();
    while let Some(p) = progress.recv().await {
        match p {
            TestProgress::LoopbackReport {
                per_s,
                lost_count,
                data_corrupted_count,
                ..
            } => {
                r.round_trips = per_s;
                r.errors += lost_count + data_corrupted_count;
            }
            TestProgress::SpeedReport {
                name,
                per_s,
                bytes_per_s,
                lost_count,
                data_corrupted_count,
                ..
            } => {
                r.errors += lost_count + data_corrupted_count;
                match name {
                    "tx_speed" => r.tx = (per_s, bytes_per_s),
                    _ => r.rx = (per_s, bytes_per_s),
                }
            }
            TestProgress::FatalError(e) => panic!("loopback test failed: {e}"),
            _ => {}
        }
    }
    device.disconnect().asynch().await.unwrap();
    r
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let key = args.next().unwrap_or_else(|| "inproc:bench/loopback".into());
    let seconds = args.next().map_or(3, |s| s.parse().expect("seconds"));
    let packet_size = args
        .next()
        .filter(|s| s != "max")
        .map(|s| s.parse().expect("packet size"));
    let rounds: usize = args.next().map_or(3, |s| s.parse().expect("rounds"));

    let direct = if let Some(path) = key.strip_prefix("inproc:") {
        start_in_process_device(path);
        ClientConfig::new().in_process_path(path.into())
    } else if let Some(rest) = key.strip_prefix("usb:") {
        let mut parts = rest.split(':');
        let vid = u16::from_str_radix(parts.next().unwrap(), 16).unwrap();
        let pid = u16::from_str_radix(parts.next().unwrap(), 16).unwrap();
        let serial = parts.next().expect("usb:<vid>:<pid>:<serial>");
        ClientConfig::new()
            .usb_vid_pid(vid, pid)
            .serial_eq(serial.into())
    } else {
        panic!("key: usb:<vid>:<pid>:<serial> or inproc:<path>");
    };

    let dir = std::env::temp_dir().join(format!("ww-hub-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let config = ww_hub::HubConfig {
        runtime_dir: dir.clone(),
        idle_exit: Duration::from_secs(600),
        linger: Duration::from_secs(1),
    };
    // the hub on a runtime of its own, one thread as `ww_hub::run_blocking` (`HUB_RT=multi`: a multi-thread one)
    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();
    let hub = std::thread::spawn(move || {
        let rt = match std::env::var("HUB_RT").as_deref() {
            Ok("multi") => tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build(),
            _ => tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build(),
        }
        .unwrap();
        rt.block_on(async move {
            let bound = ww_hub::Bound::bind(config)
                .unwrap()
                .expect("a hub of its own");
            shutdown_tx.send(bound.shutdown_handle()).unwrap();
            bound.run().await
        })
    });
    let shutdown = shutdown_rx.recv().unwrap();
    let socket = dir.join(ww_local_socket::files::HUB_SOCKET).display().to_string();
    let through_hub = ClientConfig::new().hub_device_at(socket, key.clone());

    let (mut ds, mut hs) = (vec![], vec![]);
    for _ in 0..rounds {
        // direct first: the hub opens the device afterwards, it is closed for the next direct run
        ds.push(run(direct.clone(), seconds, packet_size).await);
        hs.push(run(through_hub.clone(), seconds, packet_size).await);
        hub_closed().await;
    }
    let (d, h) = (median(ds), median(hs));
    shutdown.shutdown();
    let _ = hub.join();
    let _ = std::fs::remove_dir_all(&dir);

    let pct = |a: f32, b: f32| (b / a - 1.0) * 100.0;
    println!("{key}, {seconds} s per stage, packet size {packet_size:?}, median of {rounds}");
    println!("{:<22} {:>14} {:>14} {:>8}", "", "direct", "hub", "change");
    println!(
        "{:<22} {:>14.0} {:>14.0} {:>7.1}%",
        "round trips/s",
        d.round_trips,
        h.round_trips,
        pct(d.round_trips, h.round_trips)
    );
    for (name, a, b) in [("host → device", d.tx, h.tx), ("device → host", d.rx, h.rx)] {
        println!(
            "{:<22} {:>12.0}/s {:>12.0}/s {:>7.1}%",
            format!("{name} msgs"),
            a.0,
            b.0,
            pct(a.0, b.0)
        );
        println!(
            "{:<22} {:>10.1} kB/s {:>10.1} kB/s {:>7.1}%",
            format!("{name} bytes"),
            a.1 / 1000.0,
            b.1 / 1000.0,
            pct(a.1, b.1)
        );
    }
    println!("lost or corrupted: direct {}, hub {}", d.errors, h.errors);
}

//! Live link handover (CLT-44) between processes: `hub_dev hold-usb` holds a simulated USB device (served by this
//! process on a local socket, `usbsim:`) directly, with calls in flight all the time and a counted stream; this
//! process wants the device too. The holder hands its live link to the hub and goes on through it: no call fails, no
//! tick is lost, doubled or out of order, and the device's link never goes down or sets up again. The hub is `hub_dev`
//! too, started by whoever needs it.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use hub_reconnect::client::Client;
use wire_weaver_client::hub::HubClient;
use wire_weaver_client::{ClientKind, ClientMeta};
use ww_hub_api::DeviceStateOwned;

const HUB_DEV: &str = env!("CARGO_BIN_EXE_hub_dev");

/// A fresh runtime dir with the environment the client finds the hub by; each test runs in its own process.
fn setup(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ww-live-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // SAFETY: set before anything reads them, nextest runs each test in a process of its own
    unsafe {
        std::env::set_var(ww_local_socket::RUNTIME_DIR_ENV, &dir);
        std::env::set_var(wire_weaver_client::hub::EXE_ENV, HUB_DEV);
        // a hub a failed test leaves behind goes soon
        std::env::set_var(ww_hub::IDLE_EXIT_ENV, "5");
        // an adopted link whose program died goes soon
        std::env::set_var(ww_hub::ADOPT_ATTACH_ENV, "300");
    }
    dir
}

/// The holder in another process, and its lines as they come.
struct Holder {
    child: Child,
    lines: mpsc::Receiver<String>,
}

/// A `state` line of the holder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct State {
    ok: u32,
    failed: u32,
    slowest_us: u64,
    ticks: u32,
    out_of_order: u32,
}

impl Holder {
    fn start(socket: &Path) -> Holder {
        let mut child = Command::new(HUB_DEV)
            .args(["hold-usb", &socket.display().to_string(), "Cli"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start the holder");
        // its own lines and its log, as they come
        let (tx, lines) = mpsc::channel();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let stderr = BufReader::new(child.stderr.take().unwrap());
        let err_tx = tx.clone();
        std::thread::spawn(move || {
            for line in stdout.lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        std::thread::spawn(move || {
            for line in stderr.lines().map_while(Result::ok) {
                eprintln!("holder: {line}");
                if err_tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut holder = Holder { child, lines };
        holder.wait_for("holding", |_| {});
        holder
    }

    /// The first line starting with `prefix`; the lines before it go to `before`.
    fn wait_for(&mut self, prefix: &str, mut before: impl FnMut(&str)) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(line) if line.starts_with(prefix) => return line,
                Ok(line) => before(&line),
                Err(e) => panic!("the holder said no {prefix:?}: {e}"),
            }
        }
    }

    /// The next state line, with the lines before it.
    fn state(&mut self) -> (State, Vec<String>) {
        let mut before = vec![];
        let line = self.wait_for("state ", |l| before.push(l.to_string()));
        let n: Vec<u64> = line["state ".len()..]
            .split(' ')
            .map(|n| n.parse().unwrap())
            .collect();
        let state = State {
            ok: n[0] as u32,
            failed: n[1] as u32,
            slowest_us: n[2],
            ticks: n[3] as u32,
            out_of_order: n[4] as u32,
        };
        (state, before)
    }

    /// State lines until calls and ticks moved on by at least `calls` / `ticks` since `from`, every other line seen.
    fn advance(&mut self, from: State, calls: u32, ticks: u32) -> (State, Vec<String>) {
        let mut seen = vec![];
        loop {
            let (state, before) = self.state();
            seen.extend(before);
            if state.ok >= from.ok + calls && state.ticks >= from.ticks + ticks {
                return (state, seen);
            }
        }
    }
}

impl Drop for Holder {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn asker() -> impl FnOnce(wire_weaver_client::ClientConfig) -> wire_weaver_client::ClientConfig {
    |c| c.client_meta(ClientMeta::new().kind(ClientKind::Test).program("asker"))
}

async fn stop_hub() {
    if let Ok(hub) = HubClient::config(|c| c.hub_api().hub_reconnect(None))
        .connect()
        .await
    {
        let _ = hub.shutdown().call().await;
    }
}

/// Every `add(a, 1000)` the device saw, as a set; panics on a doubled one.
fn adds_once(seen: &std::sync::Mutex<hub_reconnect::Seen>) -> BTreeSet<u32> {
    let mut set = BTreeSet::new();
    for (a, b) in &seen.lock().unwrap().adds {
        if *b == 1000 {
            assert!(set.insert(*a), "add({a}, 1000) reached the device twice");
        }
    }
    set
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_holder_hands_its_live_link_to_the_hub() {
    let _ = tracing_subscriber::fmt::try_init();
    let dir = setup("handover");
    let socket = dir.join("dev.sock");
    let (device, seen) = hub_reconnect::start_device_usb_sim(&socket);
    let key = wire_weaver_client::usb_sim::key_for(&socket);
    let mut holder = Holder::start(&socket);
    let (before, _) = holder.state();
    holder.advance(before, 100, 10);
    assert_eq!(device.link_events(), (1, 0), "the holder's link");
    // a busy device: the holder has requests in flight all the time
    device.slow_requests(Duration::from_millis(1));
    let (before, _) = holder.state();

    // this program wants the device too: the holder hands its live link to the hub, both go on through it
    let started = Instant::now();
    let mut client = Client::config(|c| asker()(c.usb_sim_path(socket.display().to_string())))
        .connect()
        .await
        .expect("connect through the hub after the handover");
    let connect = started.elapsed();
    assert_eq!(client.add(20, 22).call().await.unwrap(), 42);

    let (after, lines) = holder.advance(before, 200, 20);
    let handed = lines
        .iter()
        .find_map(|l| {
            l.split_once("handed the live link to the ww hub in ")
                .map(|(_, r)| r)
        })
        .expect("the holder says it handed its live link over");
    let in_flight: u32 = handed
        .split_once(" (")
        .and_then(|(_, r)| r.split_once(' '))
        .and_then(|(n, _)| n.parse().ok())
        .unwrap();
    assert!(in_flight > 0, "requests were in flight: {handed}");
    for line in &lines {
        assert!(
            !line.starts_with("disconnected") && !line.starts_with("reconnected"),
            "the holder's link went on: {line}"
        );
    }
    assert_eq!(after.failed, 0, "no call failed: {after:?}");
    assert_eq!(
        after.out_of_order, 0,
        "no tick lost, doubled or out of order"
    );
    assert_eq!(
        device.link_events(),
        (1, 0),
        "the device's link never went down or set up again"
    );

    // the hub holds the device now, with a tunnel for each program
    let hub = HubClient::config(|c| c.hub_api().hub_reconnect(None))
        .connect()
        .await
        .unwrap();
    let devices = hub.registry().devices().call().await.unwrap();
    let entry = devices
        .iter()
        .find(|d| d.key == key)
        .unwrap_or_else(|| panic!("{key} in {devices:?}"));
    assert_eq!(entry.state, DeviceStateOwned::Open);
    assert_eq!(entry.tunnels, 2);
    let held = wire_weaver_client::holders::holder_of(&key).expect("the hub holds it");
    assert_eq!(held.mode, wire_weaver_client::holders::Mode::Hub);

    // every call of the holder reached the device once, none was lost
    drop(holder);
    let adds = adds_once(&seen);
    let max = *adds.last().unwrap();
    assert!(max >= after.ok, "{max} {after:?}");
    let missing: Vec<u32> = (1..=after.ok).filter(|a| !adds.contains(a)).collect();
    assert!(
        missing.is_empty(),
        "calls the device never saw: {missing:?}"
    );
    eprintln!(
        "live handover: the asker connected in {connect:?}; the holder handed over in {handed}; its slowest \
         batch of 20 calls (1 ms each on the device) {:?}",
        Duration::from_micros(after.slowest_us)
    );

    drop(client);
    stop_hub().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// Polls `ok` until it holds, panics with `what` after 3 s.
async fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !ok() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

/// The asker connecting through the hub on a task of its own: it waits while the handover runs.
fn ask(socket: &Path) -> tokio::task::JoinHandle<anyhow::Result<Client>> {
    let path = socket.display().to_string();
    tokio::spawn(async move {
        Client::config(|c| asker()(c.usb_sim_path(path.clone())))
            .connect()
            .await
    })
}

async fn hub_api() -> HubClient {
    HubClient::config(|c| c.hub_api().hub_reconnect(None))
        .connect()
        .await
        .expect("the hub API")
}

/// The holder has a busy link to the device, the hub's open of the device waits in the commit (claims held).
async fn holder_in_commit(
    socket: &Path,
    device: &tests_common::TestDevice,
) -> (
    Holder,
    State,
    tokio::task::JoinHandle<anyhow::Result<Client>>,
) {
    let mut holder = Holder::start(socket);
    let (before, _) = holder.state();
    let (_, _) = holder.advance(before, 60, 6);
    device.slow_requests(Duration::from_millis(1));
    let usb = device.usb_sim().unwrap();
    usb.hold_claims(true);
    let (before, _) = holder.state();
    let asker = ask(socket);
    wait_until("the hub's claim", || usb.claims_waiting() == 1).await;
    assert!(!usb.held(), "the holder let go of the interface");
    (holder, before, asker)
}

/// The hub dies in the commit, before it holds the device: the holder takes the device back and goes on directly.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_hub_dying_in_the_commit_leaves_the_holder_going_on() {
    let _ = tracing_subscriber::fmt::try_init();
    let dir = setup("hub-dies");
    let socket = dir.join("dev.sock");
    let (device, _seen) = hub_reconnect::start_device_usb_sim(&socket);
    let key = wire_weaver_client::usb_sim::key_for(&socket);
    let (mut holder, before, asker) = holder_in_commit(&socket, &device).await;
    let usb = device.usb_sim().unwrap();

    let pid = hub_api().await.info().call().await.unwrap().pid;
    let killed = Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    // the asker would start another hub and ask again
    asker.abort();
    // the killed hub's claim went with its connection, the holder's comes to take the device back
    wait_until("the hub's claim to go", || usb.claims_dropped() == 1).await;
    wait_until("the holder's claim", || usb.claims_waiting() == 1).await;
    usb.hold_claims(false);

    let (after, lines) = holder.advance(before, 100, 10);
    assert!(
        lines.iter().any(|l| l.contains("keeping the device")),
        "the holder kept the device: {lines:?}"
    );
    for line in &lines {
        assert!(
            !line.starts_with("disconnected") && !line.starts_with("reconnected"),
            "the holder's link went on: {line}"
        );
    }
    assert_eq!(after.failed, 0, "no call failed: {after:?}");
    assert_eq!(
        after.out_of_order, 0,
        "no tick lost, doubled or out of order"
    );
    assert_eq!(device.link_events(), (1, 0), "the device's link went on");
    assert!(usb.held(), "the holder holds the interface again");
    let held = wire_weaver_client::holders::holder_of(&key).expect("the holder holds it");
    assert_eq!(held.mode, wire_weaver_client::holders::Mode::Direct);

    drop(holder);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The holder dies in the commit: the hub has the device with its link, the asker goes on through it, and the dead
/// holder's tunnel goes after the attach timeout.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_holder_dying_in_the_commit_leaves_the_device_with_the_hub() {
    let _ = tracing_subscriber::fmt::try_init();
    let dir = setup("holder-dies");
    let socket = dir.join("dev.sock");
    let (device, _seen) = hub_reconnect::start_device_usb_sim(&socket);
    let key = wire_weaver_client::usb_sim::key_for(&socket);
    let (holder, _, asker) = holder_in_commit(&socket, &device).await;
    let usb = device.usb_sim().unwrap();

    drop(holder);
    usb.hold_claims(false);
    let mut client = tokio::time::timeout(Duration::from_secs(2), asker)
        .await
        .expect("the asker connects")
        .unwrap()
        .expect("connect through the hub");
    assert_eq!(client.add(20, 22).call().await.unwrap(), 42);
    assert_eq!(device.link_events(), (1, 0), "the device's link went on");

    let hub = hub_api().await;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let devices = hub.registry().devices().call().await.unwrap();
        let entry = devices
            .iter()
            .find(|d| d.key == key)
            .expect("the hub has it");
        assert_eq!(entry.state, DeviceStateOwned::Open);
        if entry.tunnels == 1 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the dead holder's tunnel stays: {entry:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(client.add(1, 2).call().await.unwrap(), 3);
    assert_eq!(device.link_events(), (1, 0));

    drop(client);
    drop(hub);
    stop_hub().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// Another program grabs the interface in the gap: the hub can't open the device and the holder can't take it back.
/// The holder's link ends as handed to the hub and comes back through it once the interface is free.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_interface_grabbed_in_the_gap_ends_in_a_reconnect_through_the_hub() {
    let _ = tracing_subscriber::fmt::try_init();
    let dir = setup("grabbed");
    let socket = dir.join("dev.sock");
    let (device, _seen) = hub_reconnect::start_device_usb_sim(&socket);
    let (mut holder, _, asker) = holder_in_commit(&socket, &device).await;
    let usb = device.usb_sim().unwrap();

    assert!(usb.grab(), "nobody holds the interface in the gap");
    usb.hold_claims(false);
    assert_eq!(
        holder.wait_for("disconnected", |_| {}),
        "disconnected HandedToHub"
    );
    usb.ungrab();
    holder.wait_for("reconnected", |_| {});
    let (before, _) = holder.state();
    let (after, _) = holder.state();
    let (after, _) = holder.advance(after, 40, 3);
    assert!(after.ok > before.ok, "the holder's calls work again");

    asker.abort();
    drop(holder);
    stop_hub().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// An adoption prepared on a hub API connection that then goes keeps nothing: the hub opens the device as usual.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_prepared_adoption_goes_with_its_connection() {
    use wire_weaver::shrink_wrap::SerializeShrinkWrapOwned as _;
    use wire_weaver::ww_version::{
        ApiHashPair, CompactVersion, FullVersion, GlobalTypeId, Version,
    };
    use wire_weaver_client::adopt::{LinkSnapshotOwned, MediumOwned};

    let _ = tracing_subscriber::fmt::try_init();
    let dir = setup("prepared");
    let socket = dir.join("dev.sock");
    let (device, _seen) = hub_reconnect::start_device_usb_sim(&socket);
    let key = wire_weaver_client::usb_sim::key_for(&socket);

    // this test plays a holder that prepared and died
    let hub = HubClient::config(|c| {
        c.hub_api()
            .hub_reconnect(None)
            .client_meta(ClientMeta::new().kind(ClientKind::Test).program("holder"))
    })
    .connect()
    .await;
    let hub = match hub {
        Ok(hub) => hub,
        Err(_) => {
            wire_weaver_client::hub::ensure_running(true).await.unwrap();
            hub_api().await
        }
    };
    let info = ww_link::DeviceInfo {
        dev_link_version: ww_link::LINK_VERSION,
        api_model_version: CompactVersion::new(GlobalTypeId::new(513), 0, 2, 0),
        user_api_version: FullVersion::new("test", Version::new(0, 1, 3)),
        hash: ApiHashPair::empty(),
        dev_max_message_len: 512,
        packet_accumulation_time_us: 500,
        token: 7,
    };
    let snapshot = LinkSnapshotOwned {
        client_version: wire_weaver_client::adopt::CLIENT_VERSION.into(),
        key: key.clone(),
        medium: MediumOwned::UsbSim {
            max_packet: 64,
            seq_prefix: true,
        },
        tx_frame: None,
        rx_partial: None,
        rx_staged: vec![],
        rx_next_frame: None,
        token: 7,
        device_info: info.make_owned().to_ww_bytes_owned().unwrap(),
        ping_in_ms: 1000,
        last_rx_ms_ago: 3,
        next_seq: 5,
        max_seq: 1024,
        in_flight: vec![],
        full_only: vec![],
        streams: vec![],
        observed: vec![],
    };
    let id = hub
        .adopt()
        .prepare(snapshot.encode().unwrap())
        .call()
        .await
        .expect("prepared");
    assert_ne!(id, 0);

    // the slot is kept for the commit: a program asking for the device waits
    let connecting = tokio::spawn(async move {
        Client::config(|c| {
            asker()(c.usb_sim_path(socket.display().to_string()))
                .hub(wire_weaver_client::hub::HubMode::Required)
        })
        .connect()
        .await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!connecting.is_finished(), "the device's slot is kept");
    assert_eq!(device.link_events(), (0, 0));

    // the holder dies before its commit
    tokio::task::spawn_blocking(move || drop(hub))
        .await
        .unwrap();
    let mut client = tokio::time::timeout(Duration::from_secs(2), connecting)
        .await
        .expect("the hub opens the device once the adoption went")
        .unwrap()
        .expect("connect through the hub");
    assert_eq!(client.add(20, 22).call().await.unwrap(), 42);
    assert_eq!(device.link_events(), (1, 0));

    drop(client);
    stop_hub().await;
    let _ = std::fs::remove_dir_all(&dir);
}

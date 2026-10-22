//! A polite handover to the hub (CLT-43) between two processes: `hub_dev hold` holds a simulated device (served by
//! this process on a local socket, `ipc_exclusive`) directly; this process wants it too. A command-line holder hands
//! it to the hub and goes on through it after a gap (the release-and-reopen baseline CLT-44 is measured against); a
//! GUI holder refuses and the error names it. The hub is `hub_dev` too, started by whoever needs it.

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
    let dir = std::env::temp_dir().join(format!("ww-polite-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // SAFETY: set before anything reads them, nextest runs each test in a process of its own
    unsafe {
        std::env::set_var(ww_local_socket::RUNTIME_DIR_ENV, &dir);
        std::env::set_var(wire_weaver_client::hub::EXE_ENV, HUB_DEV);
        // a hub a failed test leaves behind goes soon
        std::env::set_var(ww_hub::IDLE_EXIT_ENV, "5");
    }
    dir
}

/// The holder in another process, and its lines as they come.
struct Holder {
    child: Child,
    lines: mpsc::Receiver<String>,
}

impl Holder {
    fn start(socket: &Path, kind: &str) -> Holder {
        let mut child = Command::new(HUB_DEV)
            .args(["hold", &socket.display().to_string(), kind])
            .stdout(Stdio::piped())
            .spawn()
            .expect("start the holder");
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in stdout.lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut holder = Holder { child, lines };
        holder.wait_for("holding");
        holder
    }

    /// The first line starting with `prefix`, the lines before it skipped.
    fn wait_for(&mut self, prefix: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(line) if line.starts_with(prefix) => return line,
                Ok(_) => {}
                Err(e) => panic!("the holder said no {prefix:?}: {e}"),
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_command_line_holder_hands_the_device_to_the_hub() {
    let _ = tracing_subscriber::fmt::try_init();
    let dir = setup("handover");
    let socket = dir.join("dev.sock");
    let (_device, seen) = hub_reconnect::start_device_ipc(&socket);
    let key = format!("ipc:{}", socket.display());
    let mut holder = Holder::start(&socket, "Cli");
    let held = wire_weaver_client::holders::holder_of(&key).expect("the holder holds it");
    assert_eq!(
        (held.program.as_str(), held.kind.as_str()),
        ("polite-holder", "Cli")
    );
    assert!(!held.handover.is_empty(), "its lock file names its socket");

    // this program wants the device too: the holder hands it to the hub, both go on through it
    let started = Instant::now();
    let mut client =
        Client::config(|c| asker()(c.ipc_path(socket.display().to_string()).ipc_exclusive()))
            .connect()
            .await
            .expect("connect through the hub after the handover");
    let connect = started.elapsed();
    assert_eq!(client.add(20, 22).call().await.unwrap(), 42);

    assert_eq!(holder.wait_for("disconnected"), "disconnected HandedToHub");
    let reconnected = holder.wait_for("reconnected ");
    let (gap_us, resumed) = reconnected["reconnected ".len()..].split_once(' ').unwrap();
    let gap_us: u64 = gap_us.parse().unwrap();
    assert_eq!(resumed, "1", "the holder's stream goes on through the hub");
    holder.wait_for("ok after reconnect");
    // the release-and-reopen baseline for CLT-44's live link handover
    eprintln!(
        "polite handover: the asker connected in {connect:?}, the holder's gap was {:?}",
        Duration::from_micros(gap_us)
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
    assert!(
        wire_weaver_client::holders::holder_of(&key).is_none(),
        "nobody holds it directly any more"
    );
    assert!(seen.lock().unwrap().adds.contains(&(20, 22)));

    drop(holder);
    drop(client);
    stop_hub().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_gui_holder_refuses_and_is_named() {
    let _ = tracing_subscriber::fmt::try_init();
    let dir = setup("refusal");
    let socket = dir.join("dev.sock");
    let (_device, _seen) = hub_reconnect::start_device_ipc(&socket);
    let key = format!("ipc:{}", socket.display());
    let holder = Holder::start(&socket, "Gui");

    let e =
        match Client::config(|c| asker()(c.ipc_path(socket.display().to_string()).ipc_exclusive()))
            .connect()
            .await
        {
            Ok(_) => panic!("a GUI holder doesn't hand its device over"),
            Err(e) => format!("{e:#}"),
        };
    assert!(e.contains("polite-holder"), "{e}");
    assert!(e.contains("Gui"), "{e}");
    assert!(e.contains("doesn't hand it over"), "{e}");

    // the holder never let go: it still holds the device and its calls go on
    let held = wire_weaver_client::holders::holder_of(&key).expect("still held");
    assert_eq!(held.program, "polite-holder");
    while let Ok(line) = holder.lines.try_recv() {
        assert!(!line.starts_with("disconnected"), "{line}");
    }

    drop(holder);
    stop_hub().await;
    let _ = std::fs::remove_dir_all(&dir);
}

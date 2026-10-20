//! The tap commands end to end (TOOL-14): this CLI's own connection to a device over simulated USB (64 B packets,
//! the real framers on both ends), printed live the way `ww monitor` prints it, captured the way `ww capture` writes
//! it, and the capture read back the way `ww explain FILE` reads it: the same text.

use super::*;
use crate::util::resolve;
use wire_weaver::shrink_wrap::UNib32;
use wire_weaver_client::observe::ObserveMode;
use wire_weaver_client::ww_numeric::NumericValue;
use wire_weaver_client::ww_self::ValueOwned;
use wire_weaver_client::{ClientConfig, DynResource};

mod server {
    use tests_common::TestProcessEventsAsync;
    use wire_weaver::prelude::*;
    use ww_stream::observe::ObserveLimits;
    use ww_user_info::{UserInfoCaps, UserInfoError};

    pub struct Server {
        pub blob: [u8; 512],
        pub speed: u16,
        /// Counts up at every poll: the observed `load` changes each time
        pub load: u16,
        pub name: String,
        pub observe: api_impl::ObserveState,
        pub buf: Vec<u8>,
    }

    impl Server {
        pub fn new(speed: u16) -> Self {
            Server {
                blob: core::array::from_fn(|i| i as u8),
                speed,
                load: 1000,
                name: "tapped".into(),
                observe: api_impl::ObserveState::new(ObserveLimits::DEFAULT),
                buf: vec![0; 512],
            }
        }

        async fn add(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            a: u32,
            b: u32,
        ) -> RpcResult<u32> {
            Ready(a.wrapping_add(b))
        }

        async fn blob(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            len: u16,
        ) -> RpcResult<RefVec<'_, u8>> {
            Ready(RefVec::new_bytes(&self.blob[..(len as usize).min(512)]))
        }

        async fn get_speed(&mut self, _cx: &mut Context<'_, impl EventOut>) -> GetResult<u16, ()> {
            Value(self.speed)
        }

        async fn set_speed(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            value: u16,
        ) -> SetResult<()> {
            self.speed = value;
            Set
        }

        async fn get_load(&mut self, _cx: &mut Context<'_, impl EventOut>) -> GetResult<u16, ()> {
            Value(self.load)
        }

        async fn get_state(&mut self, _cx: &mut Context<'_, impl EventOut>) -> GetResult<u8, ()> {
            Value(3)
        }

        async fn set_user_name(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            value: MaxLen<&'_ str, 32>,
        ) -> SetResult<UserInfoError<'_>> {
            self.name = value.to_string();
            Set
        }

        async fn get_user_name(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
        ) -> GetResult<MaxLen<&'_ str, 32>, UserInfoError<'_>> {
            Value(MaxLen::new_unchecked(&self.name))
        }

        async fn set_user_notes(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            _value: MaxLen<&'_ str, 256>,
        ) -> SetResult<UserInfoError<'_>> {
            Unimplemented.into()
        }

        async fn get_user_notes(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
        ) -> GetResult<MaxLen<&'_ str, 256>, UserInfoError<'_>> {
            Unimplemented.into()
        }

        fn observe_state(&mut self) -> &mut api_impl::ObserveState {
            &mut self.observe
        }
    }

    impl api_impl::TapDemoConsts for Server {
        const USER_CAPABILITIES: UserInfoCaps = UserInfoCaps {
            name_max: 32,
            notes_max: 0,
            persistent: false,
        };
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            tap_api :: TapDemo for super::Server,
            server = true, no_alloc = true, use_async = true,
            method_model = "_=immediate",
            property_model = "_=get_set",
        );
    }

    impl TestProcessEventsAsync for Server {
        type Medium = ();

        async fn process_request_bytes<'a>(
            &mut self,
            bytes: &[u8],
            scratch: &'a mut [u8],
            out: &mut impl EventOut,
            medium: (),
        ) -> Result<&'a [u8], ShrinkWrapError> {
            self.process_request_bytes(bytes, scratch, out, medium)
                .await
        }

        async fn poll(&mut self, out: &mut impl EventOut, now_ms: u64) {
            let mut buf = core::mem::take(&mut self.buf);
            // the device took a new reading
            self.load += 1;
            self.mark_changed_load();
            _ = self.poll_properties(out, (), now_ms * 1000, &mut buf).await;
            self.buf = buf;
        }
    }
}

/// A demo device on simulated USB.
fn demo_device(path: &str, speed: u16) -> tests_common::TestDevice {
    tests_common::start_device_async_usb(
        path,
        64,
        server::Server::new(speed),
        tap_api::TAP_DEMO_FULL_GID,
        server::api_impl::api_hash(),
    )
}

fn u32v(v: u32) -> ValueOwned {
    ValueOwned::Numeric(NumericValue::U32(v))
}

fn views() -> [ViewArgs; 3] {
    [
        ViewArgs {
            layer: Layer::Frame,
            bytes: true,
            bits: false,
        },
        ViewArgs {
            layer: Layer::Link,
            bytes: false,
            bits: false,
        },
        ViewArgs {
            layer: Layer::Api,
            bytes: false,
            bits: true,
        },
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn monitor_capture_and_explain_file_agree() {
    let device = demo_device("cli/tap", 7);
    // as main.rs does: the tap first, then the connection that records into it
    let tap = LiveTap::new(Layer::Frame);
    let client = DynClient::from_config(
        ClientConfig::new()
            .in_process_path(device.path())
            .tap(tap.recorder.clone()),
    )
    .connect()
    .await
    .expect("connect");
    tap.add_bundles(&client);

    let root = DynResource::root(client.device_introspect().unwrap().api_bundle.clone());
    let add = resolve(root.clone(), "add").unwrap();
    let blob = resolve(root, "blob").unwrap();
    // several requests at once share host packets
    let calls: Vec<_> = (0..8u32)
        .map(|i| {
            let (add, cmd) = (add.clone(), client.cmd().clone());
            tokio::spawn(async move { add.call(&cmd, &[u32v(i), u32v(100)]).await })
        })
        .collect();
    for (i, c) in calls.into_iter().enumerate() {
        let sum = c.await.unwrap().unwrap().unwrap();
        assert_eq!(sum, u32v(i as u32 + 100));
    }
    // a 300 B reply goes out in several device packets
    let bytes = blob
        .call(client.cmd(), &[ValueOwned::Numeric(NumericValue::U16(300))])
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(bytes, ValueOwned::Vec(v) if v.len() == 300));

    client.disconnect().asynch().await.unwrap();
    let (mut feed, recorder) = LiveFeed::new(tap);
    recorder.flush();

    // live: every view printed as the blocks come, and the blocks written as `ww capture` writes them
    let mut monitors = views().map(|v| Monitor::new(v, false));
    let mut live = [vec![], vec![], vec![]];
    let mut file = vec![];
    let dir = std::env::temp_dir().join(format!("ww_cli_tap_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cap.wwtape");
    let _ = std::fs::remove_file(&path);
    let mut writer = TapeWriter::create(&path, SyncPolicy::default()).unwrap();
    let stop = AtomicBool::new(true);
    consume(&mut feed, &stop, |b| {
        file.extend_from_slice(b.bytes());
        writer.write_block(b.bytes())?;
        for (m, out) in monitors.iter_mut().zip(&mut live) {
            m.block(b, out)?;
        }
        Ok(())
    })
    .unwrap();
    for (m, out) in monitors.iter_mut().zip(&mut live) {
        m.finish(out).unwrap();
    }
    // `ww stats`: the framer's view of the same link
    let report = monitors[0].stats_report();
    assert!(report.contains("1 split over 5 frames"), "{report}");
    assert!(report.contains("add: 8"), "{report}");
    let live = live.map(|b| String::from_utf8(b).unwrap());

    // `ww capture` wrote the same bytes, `ww explain FILE` prints the same text
    assert_eq!(writer.finish().unwrap(), path);
    assert_eq!(std::fs::read(&path).unwrap(), file);
    for (view, live) in views().into_iter().zip(&live) {
        let mut replayed = vec![];
        replay(
            &mut FileFeed::open(&path, false).unwrap(),
            view,
            false,
            &mut replayed,
        )
        .unwrap();
        assert_eq!(&String::from_utf8(replayed).unwrap(), live);
    }
    std::fs::remove_dir_all(&dir).unwrap();

    let [frames, link, api] = &live;
    assert!(frames.contains("session Host InProcessUsb"), "{frames}");
    assert!(frames.contains("frame #"), "{frames}");
    // the 300 B reply in several frames, a packet with several requests
    assert!(frames.contains("Start Data0 "), "{frames}");
    assert!(
        frames
            .lines()
            .any(|l| l.contains("→ frame #") && l.matches("Full Data0").count() >= 2),
        "{frames}"
    );
    assert!(
        link.contains("LinkSetup") && link.contains("LinkReady"),
        "{link}"
    );
    assert!(!link.contains("frame #"), "{link}");
    // replies explained by the request their seq pairs them with: no --for needed
    assert!(api.contains("Request Call add  a: 3, b: 100"), "{api}");
    assert!(
        api.lines()
            .any(|l| l.contains("Event Value add  103") && l.contains("reply to #")),
        "{api}"
    );
    assert!(api.contains("bits in values"), "the --bits table: {api}");
    assert!(!api.contains("LinkSetup"), "{api}");
}

#[test]
fn sync_policies() {
    let p = |s: &str| s.parse::<Sync>().map(|s| s.0);
    assert_eq!(p("none"), Ok(SyncPolicy::None));
    assert_eq!(p("blocks:16"), Ok(SyncPolicy::EveryBlocks(16)));
    assert_eq!(p("1s"), Ok(SyncPolicy::Interval(Duration::from_secs(1))));
    assert_eq!(
        p("250ms"),
        Ok(SyncPolicy::Interval(Duration::from_millis(250)))
    );
    assert!(p("0s").is_err() && p("fast").is_err() && p("blocks:x").is_err());
}

/// clap's own checks of the command tree (unique argument ids, ..), which a release build skips.
#[test]
fn cli_is_consistent() {
    <crate::cli::Cli as clap::CommandFactory>::command().debug_assert();
}

/// A `Write` the test reads back.
#[derive(Clone, Default)]
struct Shared(Arc<std::sync::Mutex<Vec<u8>>>);

impl Write for Shared {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// `--monitor=api --capture FILE` on a command's own client: the request and its paired reply printed, and the file
/// read back by `ww explain FILE` as the same text.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn monitor_and_capture_flags_on_a_commands_own_client() {
    let device = demo_device("cli/tap_flags", 7);
    let dir = std::env::temp_dir().join(format!("ww_cli_tap_flags_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cmd.wwtape");
    let _ = std::fs::remove_file(&path);
    let flags = TapFlags {
        monitor: Some(Layer::Api),
        tap_bytes: false,
        tap_bits: false,
        capture: Some(path.clone()),
    };
    let printed = Shared::default();
    // as main.rs does: the tap first, the client records into it
    let attached = Attached::start_with(&flags, Box::new(printed.clone())).unwrap();
    let client = DynClient::from_config(
        ClientConfig::new()
            .in_process_path(device.path())
            .tap(attached.recorder()),
    )
    .connect()
    .await
    .expect("connect");
    attached.add_bundles(&client);
    let root = DynResource::root(client.device_introspect().unwrap().api_bundle.clone());
    let add = resolve(root, "add").unwrap();
    let sum = add
        .call(client.cmd(), &[u32v(3), u32v(100)])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(sum, u32v(103));
    client.disconnect().asynch().await.unwrap();
    attached.finish().unwrap();

    let printed = String::from_utf8(printed.0.lock().unwrap().clone()).unwrap();
    assert!(
        printed.contains("Request Call add  a: 3, b: 100"),
        "{printed}"
    );
    assert!(
        printed
            .lines()
            .any(|l| l.contains("Event Value add  103") && l.contains("reply to #")),
        "{printed}"
    );
    assert!(!printed.contains("frame #"), "{printed}");
    // in the order it happened: the request line, then the reply to it
    assert!(
        printed.find("Request Call add").unwrap() < printed.find("Event Value add").unwrap(),
        "{printed}"
    );

    // the capture is a capture, and prints as it did live
    assert!(is_capture(path.to_str().unwrap()));
    let view = ViewArgs {
        layer: Layer::Api,
        bytes: false,
        bits: false,
    };
    let mut replayed = vec![];
    replay(
        &mut FileFeed::open(&path, false).unwrap(),
        view,
        false,
        &mut replayed,
    )
    .unwrap();
    assert_eq!(String::from_utf8(replayed).unwrap(), printed);

    // a second run to the same file is refused before connecting
    let err = Attached::start_with(&flags, Box::new(std::io::sink()))
        .err()
        .unwrap();
    assert!(err.to_string().contains("exists"), "{err}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A live tap on `device` printed as `--monitor=api` does and captured to a file: the monitor's text and the file.
async fn tapped_session(
    name: &str,
    device: &tests_common::TestDevice,
    traits_only: bool,
    session: impl AsyncFnOnce(&DynClient),
) -> (String, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("ww_cli_tap_{name}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cap.wwtape");
    let _ = std::fs::remove_file(&path);
    let flags = TapFlags {
        monitor: Some(Layer::Api),
        tap_bytes: false,
        tap_bits: false,
        capture: Some(path.clone()),
    };
    let printed = Shared::default();
    let attached = Attached::start_with(&flags, Box::new(printed.clone())).unwrap();
    let config = ClientConfig::new()
        .in_process_path(device.path())
        .tap(attached.recorder());
    let config = if traits_only {
        config.traits_only()
    } else {
        config
    };
    let client = DynClient::from_config(config)
        .connect()
        .await
        .expect("connect");
    attached.add_bundles(&client);
    session(&client).await;
    client.disconnect().asynch().await.unwrap();
    attached.finish().unwrap();
    let printed = String::from_utf8(printed.0.lock().unwrap().clone()).unwrap();
    (printed, path)
}

/// What `ww explain FILE` prints of a capture.
fn replayed(path: &std::path::Path, layer: Layer) -> String {
    replayed_view(path, layer, false)
}

fn replayed_view(path: &std::path::Path, layer: Layer, bytes: bool) -> String {
    let view = ViewArgs {
        layer,
        bytes,
        bits: false,
    };
    let mut out = vec![];
    replay(
        &mut FileFeed::open(path, false).unwrap(),
        view,
        false,
        &mut out,
    )
    .unwrap();
    String::from_utf8(out).unwrap()
}

/// TOOL-9: an `Observe` and its answer read as property names with the modes asked and granted, every update as the
/// properties it carries with their values (two in the first one, as the device sends them together), all of it
/// the same from a capture.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn observe_requests_replies_and_updates_are_decoded() {
    let device = demo_device("cli/tap_observe", 7);
    let (printed, path) = tapped_session("observe", &device, false, async |client| {
        let root = DynResource::root(client.device_introspect().unwrap().api_bundle.clone());
        let level = resolve(root, "").unwrap();
        let props = level.observable_properties().unwrap();
        let names: Vec<_> = props.iter().map(|(_, n, _)| n.as_str()).collect();
        assert_eq!(names, ["load", "state"]);
        let entries = props
            .iter()
            .map(|(i, ..)| (*i, ObserveMode::OnChange { min_us: UNib32(0) }))
            .collect();
        let mut observer = level.observe(client.cmd(), entries).await.unwrap();
        // the first update has both properties, later ones only what changed
        for _ in 0..3 {
            observer.recv().await.unwrap();
        }
    })
    .await;
    let lines =
        |what: &str| -> Vec<&str> { printed.lines().filter(|l| l.contains(what)).collect() };
    assert!(
        lines("Request Observe")[0].contains("load: on change; state: on change; others off"),
        "{printed}"
    );
    let granted = lines("Event Value")[0];
    assert!(
        granted.contains("load: on change") && granted.contains("budget "),
        "{printed}"
    );
    let updates = lines("Event PropertyUpdate");
    assert!(updates.len() >= 3, "{printed}");
    // typed values, not hex
    assert!(
        updates[0].contains("load: 1") && updates[0].contains("state: 3"),
        "{printed}"
    );
    assert!(
        updates.iter().all(|l| !l.contains("0x01")),
        "no raw hex: {printed}"
    );
    // the load changes at every poll, the state never: later updates carry the load alone
    assert!(
        updates[2].contains("load: 1") && !updates[2].contains("state"),
        "{printed}"
    );
    assert_eq!(replayed(&path, Layer::Api), printed);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

/// TOOL-9: calls through a global trait id (`ww info` and friends) are named by the trait's crate and decoded from
/// the snapshot the host carries, with and without the device API (a command that never introspects).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn global_trait_calls_are_named_and_decoded() {
    for (traits_only, name) in [(false, "global"), (true, "global_traits_only")] {
        let device = demo_device(&format!("cli/tap_{name}"), 7);
        let (printed, path) = tapped_session(name, &device, traits_only, async |client| {
            let user = ww_report::clients::UserInfoClient::new(client);
            let n = user.read_name().read().await.unwrap();
            assert_eq!(&*n, "tapped");
        })
        .await;
        assert!(
            printed.contains("Request Read ww_user_info::UserInfo.name"),
            "{printed}"
        );
        assert!(
            printed.lines().any(|l| l
                .contains("Event Value ww_user_info::UserInfo.name  \"tapped\"")
                && l.contains("reply to #")),
            "{printed}"
        );
        assert_eq!(replayed(&path, Layer::Api), printed, "{name}");
        // the call went out by the trait's ww_global id (API-20): 6 bytes, the crate name alone would take 12; the
        // name shown comes from the registry
        let with_bytes = replayed_view(&path, Layer::Api, true);
        let mut lines = with_bytes.lines();
        lines.find(|l| l.contains("Request Read ww_user_info::UserInfo.name"));
        let request_len = lines.next().unwrap().split_whitespace().skip(1).count();
        assert_eq!(request_len, 6, "{with_bytes}");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}

/// TOOL-14: SIGTERM (a `timeout`, a service manager) ends a command with `--capture` like Ctrl-C does: the capture is
/// finished and renamed, not left as FILE.partial.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sigterm_finishes_the_capture_file() {
    let device = demo_device("cli/tap_sigterm", 7);
    let dir = std::env::temp_dir().join(format!("ww_cli_tap_sigterm_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cap.wwtape");
    let _ = std::fs::remove_file(&path);
    let flags = TapFlags {
        monitor: None,
        tap_bytes: false,
        tap_bits: false,
        capture: Some(path.clone()),
    };
    let attached = Attached::start_with(&flags, Box::new(std::io::sink())).unwrap();
    // as main.rs does: installed before connecting
    let stop = crate::util::StopSignal::install();
    let client = DynClient::from_config(
        ClientConfig::new()
            .in_process_path(device.path())
            .tap(attached.recorder()),
    )
    .connect()
    .await
    .expect("connect");
    attached.add_bundles(&client);

    // a command that does not end by itself, until the signal
    let own_pid = std::process::id().to_string();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let status = std::process::Command::new("kill")
            .args(["-TERM", &own_pid])
            .status()
            .unwrap();
        assert!(status.success());
    });
    let root = DynResource::root(client.device_introspect().unwrap().api_bundle.clone());
    let level = resolve(root, "").unwrap();
    let entries = [(3, ObserveMode::OnChange { min_us: UNib32(0) })].into();
    let mut observer = level.observe(client.cmd(), entries).await.unwrap();
    let ended = crate::util::guard(Some(stop), async {
        loop {
            observer.recv().await.unwrap();
        }
    })
    .await;
    assert_eq!(ended, None::<()>, "the signal ended the command");

    client.disconnect().asynch().await.unwrap();
    attached.finish().unwrap();
    assert!(path.exists(), "renamed");
    assert!(
        !path.with_file_name("cap.wwtape.partial").exists(),
        "not left partial"
    );
    assert!(is_capture(path.to_str().unwrap()));
    let text = replayed(&path, Layer::Api);
    assert!(text.contains("Request Observe"), "{text}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A WHAT written like a file that is none is an error before any device is asked.
#[test]
fn explain_of_a_missing_capture_is_an_error_not_a_connect() {
    use clap::{CommandFactory, FromArgMatches};
    let explain = |args: &[&str]| {
        let matches = crate::cli::Cli::command()
            .try_get_matches_from(std::iter::once(&"ww").chain(args))
            .unwrap();
        match crate::cli::Cli::from_arg_matches(&matches).unwrap().command {
            crate::cli::Commands::Explain(args) => args.check(),
            _ => unreachable!(),
        }
    };
    let err = explain(&["explain", "/nonexistent/dir/run.wwtape"]).unwrap_err();
    assert!(err.to_string().contains("no such capture file"), "{err}");
    assert!(explain(&["explain", "run.wwtape"]).is_err());
    // names of types and resources, a file given with --file, hex: not files
    for ok in [
        &["explain", "u8", "05"][..],
        &["explain", "motor.speed"],
        &["explain", "request", "--file", "/nonexistent/x.bin"],
    ] {
        assert!(explain(ok).is_ok(), "{ok:?}");
    }
}

/// What the command line says, parsed and checked the way `run` does.
fn check(args: &[&str]) -> Result<(), String> {
    use clap::{CommandFactory, FromArgMatches};
    let matches = crate::cli::Cli::command()
        .try_get_matches_from(std::iter::once(&"ww").chain(args))
        .map_err(|e| e.to_string())?;
    let cli = crate::cli::Cli::from_arg_matches(&matches).map_err(|e| e.to_string())?;
    cli.check_tap().map_err(|e| e.to_string())
}

#[test]
fn tap_flags_belong_to_commands_that_connect() {
    assert_eq!(check(&["info"]), Ok(()));
    for ok in [
        &["--monitor", "info"][..],
        &["info", "--monitor=api", "--monitor-bits"],
        &["--monitor=frame", "--monitor-bytes", "counters"],
        &["info", "--capture", "x.wwtape"],
        &["observe", "--monitor=link", "a"],
    ] {
        assert_eq!(check(ok), Ok(()), "{ok:?}");
    }
    // `--monitor info` is not a layer called info
    assert!(
        check(&["--monitor", "list"])
            .unwrap_err()
            .contains("connection")
    );
    for refused in [
        &["--monitor", "list"][..],
        &["--capture", "x", "api", "tree", "a"],
        &["--monitor", "udev"],
        &["--monitor", "config", "show"],
        &["--monitor", "elf", "a"],
        &["--monitor=api", "explain", "u8", "00"],
        &["--monitor", "monitor"],
        &["--capture", "x", "capture", "-o", "y"],
    ] {
        assert!(check(refused).is_err(), "{refused:?}");
    }
    assert!(
        check(&["--monitor-bytes", "info"])
            .unwrap_err()
            .contains("--monitor")
    );
    // the tap commands keep their own options
    assert_eq!(
        check(&["monitor", "--layer", "api", "--bytes", "--bits"]),
        Ok(())
    );
}

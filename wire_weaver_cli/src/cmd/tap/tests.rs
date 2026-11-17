//! The tap commands end to end (TOOL-14): this CLI's own connection to a device over simulated USB (64 B packets,
//! the real framers on both ends), printed live the way `ww monitor` prints it, captured the way `ww capture` writes
//! it, and the capture read back the way `ww explain FILE` reads it: the same text.

use super::test_support::{Shared, replayed, replayed_view, tapped_session};
use super::*;
use wire_weaver::shrink_wrap::UNib32;
use wire_weaver_client::observe::ObserveMode;
use wire_weaver_client::ww_client_server::ww_si::Micros;
use wire_weaver_client::ww_numeric::NumericValue;
use wire_weaver_client::ww_self::ValueOwned;
use wire_weaver_client::{ClientConfig, DynResource};

mod server {
    use tests_common::TestProcessEventsAsync;
    use wire_weaver::prelude::*;
    use ww_stream::observe::ObserveLimits;

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

        async fn get_speed(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
        ) -> GetResult<u16, CommonError<'_>> {
            Value(self.speed)
        }

        async fn set_speed(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            value: u16,
        ) -> SetResult<CommonError<'_>> {
            self.speed = value;
            Set
        }

        async fn get_load(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
        ) -> GetResult<u16, CommonError<'_>> {
            Value(self.load)
        }

        async fn get_state(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
        ) -> GetResult<u8, CommonError<'_>> {
            Value(3)
        }

        async fn set_user_name(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            value: MaxLen<&'_ str, 32>,
        ) -> SetResult<CommonError<'_>> {
            self.name = value.to_string();
            Set
        }

        async fn get_user_name(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
        ) -> GetResult<MaxLen<&'_ str, 32>, CommonError<'_>> {
            Value(MaxLen::new_unchecked(&self.name))
        }

        fn observe_state(&mut self) -> &mut api_impl::ObserveState {
            &mut self.observe
        }
    }

    impl api_impl::TapDemoConsts for Server {
        const USER_CAPABILITIES: u8 = 32;
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
        tap_api::TAP_DEMO_WIRE_VERSION,
        tests_common::api!(server::api_impl),
    )
}

/// The framework's own global trait, `ww_timebase::Clock` (ww_global id 24), attached to a minimal device: TOOL-9's
/// framework-owned cover of a global trait call named and decoded from a snapshot (R7), with no `ww_report`.
mod global_clock {
    use tests_common::TestProcessEventsAsync;
    use wire_weaver::prelude::*;
    use wire_weaver_client::{Commander, DynClient};
    use ww_timebase::{ClockInfo, DateTime, Second, SyncConfig, SyncState, SyncStatus};

    pub struct Server {
        pub started: std::time::Instant,
    }

    impl Server {
        async fn clock_now(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<Second> {
            Ready(Second::micro(self.started.elapsed().as_micros() as u32))
        }

        async fn clock_info(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
        ) -> RpcResult<ClockInfo<'_>> {
            Ready(ClockInfo {
                resolution: Second::micro(1u32),
                source: MaxLen::new_unchecked("test clock"),
                sync_formats: MaxLen::new_unchecked(RefVec::Slice { slice: &[] }),
            })
        }

        async fn clock_time_map(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
            Ready(())
        }

        async fn close_clock_time_map(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

        async fn sideband_clock_time_map(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            _sideband: ww_client_server::StreamSideband,
        ) -> Option<ww_client_server::StreamSideband> {
            None
        }

        async fn clock_configure_sync(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            _config: SyncConfig,
        ) -> RpcResult<Result<(), CommonError<'_>>> {
            Ready(Err(CommonError::Unsupported))
        }

        async fn clock_sync_config(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
        ) -> RpcResult<SyncConfig> {
            Ready(SyncConfig::Off)
        }

        async fn clock_sync_status(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
        ) -> RpcResult<SyncStatus<'_>> {
            Ready(SyncStatus {
                state: SyncState::Free,
                offset: None,
                last_lock: None,
                source: None,
                error: None,
                slew: None,
                steerer: None,
                noted: None,
            })
        }

        async fn clock_set_time_of_day(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            _t: DateTime,
        ) -> RpcResult<Result<(), CommonError<'_>>> {
            Ready(Err(CommonError::WrongState {
                reason: MaxLen::new_unchecked("not a sync master"),
            }))
        }
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            io_timebase_api :: IoTimebaseTest for super::Server,
            server = true, no_alloc = true, use_async = true,
            method_model = "_=immediate",
            property_model = "_=get_set",
            omit = ["clock.host_sync"],
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
    }

    /// A device whose only resource is `ww_timebase::Clock`, attached globally.
    pub fn demo_device(path: &str) -> tests_common::TestDevice {
        tests_common::start_device_async_usb(
            path,
            64,
            Server {
                started: std::time::Instant::now(),
            },
            io_timebase_api::IO_TIMEBASE_TEST_FULL_GID,
            io_timebase_api::IO_TIMEBASE_TEST_WIRE_VERSION,
            tests_common::api!(api_impl),
        )
    }

    /// A client of `ww_timebase::Clock` through a global attachment, built here (TOOL-9): no pregenerated
    /// `ww_report` client, just `ww_codegen!` on the same trait the device implements.
    pub struct ClockClient {
        cmd: Commander,
    }

    impl ClockClient {
        pub fn new(device: &DynClient) -> Self {
            Self {
                cmd: device
                    .global_attachment(ww_timebase::CLOCK_FULL_GID, "Clock")
                    .cmd_tx_take(),
            }
        }
    }

    mod clock_client_impl {
        use super::*;
        wire_weaver::ww_codegen!(
            ww_timebase :: Clock for super::ClockClient,
            client = "trait_client",
            no_alloc = false,
            use_async = true,
            omit = ["clock.host_sync"],
        );
    }
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
            .resolver(tests_common::resolver())
            .tap(tap.recorder.clone()),
    )
    .connect()
    .await
    .expect("connect");
    tap.add_bundles(&client);

    let root = DynResource::root(client.device_introspect().unwrap().api_bundle.clone());
    let add = root.walk("add").unwrap();
    let blob = root.walk("blob").unwrap();
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
    let mut monitors = views().map(|v| Monitor::new(v, false, tests_common::resolver()));
    let mut live = [vec![], vec![], vec![]];
    let mut file = vec![];
    let dir = std::env::temp_dir().join(format!("ww_cli_tap_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cap.wwtape");
    let _ = std::fs::remove_file(&path);
    let mut writer = TapeWriter::create(&path, SyncPolicy::default()).unwrap();
    // the writer's own session holds its layout, ahead of the tap's blocks
    let own = writer.session();
    let stop = AtomicBool::new(true);
    consume(&mut feed, &stop, |b| {
        // full repeats are left out on both sides: the file holds the tap's and the writer's own
        if !b.is_repeat() {
            file.extend_from_slice(b.bytes());
        }
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

    // `ww capture` wrote the same blocks, besides its own layout, full repeats, index, finish and tail; `ww explain
    // FILE` prints the same text
    assert_eq!(writer.finish().unwrap().path, path);
    let written = std::fs::read(&path).unwrap();
    let mut blocks = wire_weaver_client::ww_tape::FileBlocks::new(&written[..]);
    let mut producer = vec![];
    while let Some(b) = blocks.next_block() {
        let b = b.unwrap();
        if !b.is_repeat()
            && !wire_weaver_client::ww_tape::index::is_structural(b.kind)
            && b.session != own
        {
            producer.extend_from_slice(b.bytes());
        }
    }
    assert_eq!(producer, file);
    for (view, live) in views().into_iter().zip(&live) {
        let mut replayed = vec![];
        replay(
            &mut FileFeed::open(&path, false).unwrap(),
            view,
            false,
            tests_common::resolver(),
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
    let attached =
        Attached::start_with(&flags, Box::new(printed.clone()), tests_common::resolver()).unwrap();
    let client = DynClient::from_config(
        ClientConfig::new()
            .in_process_path(device.path())
            .resolver(tests_common::resolver())
            .tap(attached.recorder()),
    )
    .connect()
    .await
    .expect("connect");
    attached.add_bundles(&client);
    let root = DynResource::root(client.device_introspect().unwrap().api_bundle.clone());
    let add = root.walk("add").unwrap();
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
        tests_common::resolver(),
        &mut replayed,
    )
    .unwrap();
    assert_eq!(String::from_utf8(replayed).unwrap(), printed);

    // a second run to the same file is refused before connecting
    let err = Attached::start_with(&flags, Box::new(std::io::sink()), tests_common::resolver())
        .err()
        .unwrap();
    assert!(err.to_string().contains("exists"), "{err}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// TOOL-9: an `Observe` and its answer read as property names with the modes asked and granted, every update as the
/// properties it carries with their values (two in the first one, as the device sends them together), all of it
/// the same from a capture.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn observe_requests_replies_and_updates_are_decoded() {
    let device = demo_device("cli/tap_observe", 7);
    let (printed, path) = tapped_session(
        "observe",
        device.path(),
        tests_common::resolver(),
        false,
        async |client| {
            let root = DynResource::root(client.device_introspect().unwrap().api_bundle.clone());
            let level = root.walk("").unwrap();
            let props = level.observable_properties().unwrap();
            let names: Vec<_> = props.iter().map(|(_, n, _)| n.as_str()).collect();
            assert_eq!(names, ["load", "state"]);
            let entries = props
                .iter()
                .map(|(i, ..)| {
                    (
                        *i,
                        ObserveMode::OnChange {
                            min: Micros::new(UNib32(0)),
                        },
                    )
                })
                .collect();
            let mut observer = level.observe(client.cmd(), entries).await.unwrap();
            // the first update has both properties, later ones only what changed
            for _ in 0..3 {
                observer.recv().await.unwrap();
            }
        },
    )
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
    // typed values, not hex: the load as a decimal number. The device counts it up from 1000 at every poll, once a
    // millisecond of real time, so its value says how long the session took and is never compared with a fixed one.
    let load = |line: &str| -> u16 {
        let digits: String = line
            .split("load: ")
            .nth(1)
            .unwrap_or_else(|| panic!("no load in {line}"))
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        digits
            .parse()
            .unwrap_or_else(|_| panic!("load not decimal in {line}"))
    };
    assert!(updates[0].contains("state: 3"), "{printed}");
    assert!(load(updates[0]) > 1000, "{printed}");
    assert!(
        updates.iter().all(|l| !l.contains("0x")),
        "no raw hex: {printed}"
    );
    // the load changes at every poll, the state never: later updates carry the load alone, counted up
    assert!(!updates[2].contains("state"), "{printed}");
    assert!(load(updates[2]) > load(updates[0]), "{printed}");
    assert_eq!(
        replayed(&path, Layer::Api, tests_common::resolver()),
        printed
    );
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

/// TOOL-9: a call through a global trait id (`ww info` and friends) is named by the trait's crate and decoded from
/// the snapshot the host carries, without the device API (a command that never introspects). With the device API
/// (here from the local cache codegen fills) the client sends the trait's absolute path instead (SRV-53), named by
/// the device's API. The framework's own cover: `ww_timebase::Clock` (ww_global id 24), with a client generated right
/// here. The `ww_user_info` variant through `ww_report` is in `ww_std_cli`'s tests.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn global_trait_calls_are_named_and_decoded() {
    for (traits_only, name) in [(false, "global"), (true, "global_traits_only")] {
        let device = global_clock::demo_device(&format!("cli/tap_{name}"));
        let (printed, path) = tapped_session(
            name,
            device.path(),
            tests_common::resolver(),
            traits_only,
            async |client| {
                let clock = global_clock::ClockClient::new(client);
                clock.now().call().await.unwrap();
            },
        )
        .await;
        let resource = match traits_only {
            true => "ww_timebase::Clock.now",
            false => "clock.now",
        };
        assert!(
            printed.contains(&format!("Request Call {resource}")),
            "{printed}"
        );
        assert!(
            printed
                .lines()
                .any(|l| l.contains(&format!("Event Value {resource}")) && l.contains("reply to #")),
            "{printed}"
        );
        assert_eq!(
            replayed(&path, Layer::Api, tests_common::resolver()),
            printed,
            "{name}"
        );
        if traits_only {
            // the call went out by the trait's ww_global id (API-20), named from the registry
            let with_bytes = replayed_view(&path, Layer::Api, true, tests_common::resolver());
            let mut lines = with_bytes.lines();
            lines.find(|l| l.contains(&format!("Request Call {resource}")));
            let request_len = lines.next().unwrap().split_whitespace().skip(1).count();
            assert!(request_len > 0, "{with_bytes}");
        }
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
    let attached =
        Attached::start_with(&flags, Box::new(std::io::sink()), tests_common::resolver()).unwrap();
    // as main.rs does: installed before connecting
    let stop = crate::util::StopSignal::install();
    let client = DynClient::from_config(
        ClientConfig::new()
            .in_process_path(device.path())
            .resolver(tests_common::resolver())
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
    let level = root.walk("").unwrap();
    let entries = [(
        3,
        ObserveMode::OnChange {
            min: Micros::new(UNib32(0)),
        },
    )]
    .into();
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
    let text = replayed(&path, Layer::Api, tests_common::resolver());
    assert!(text.contains("Request Observe"), "{text}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A WHAT written like a file that is none is an error before any device is asked.
#[test]
fn explain_of_a_missing_capture_is_an_error_not_a_connect() {
    let explain = |args: &[&str]| check(args).map_err(anyhow::Error::msg);
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

/// What the command line says, parsed and checked the way the registry's run loop does before anything connects.
fn check(args: &[&str]) -> Result<(), String> {
    let matches = crate::cli::root_command()
        .try_get_matches_from(std::iter::once(&"ww").chain(args))
        .map_err(|e| e.to_string())?;
    crate::registry::check(&matches)
        .map(|_| ())
        .map_err(|e| e.to_string())
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

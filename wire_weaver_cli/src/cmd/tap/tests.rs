//! The tap commands end to end (TOOL-14): this CLI's own connection to a device over simulated USB (64 B packets,
//! the real framers on both ends), printed live the way `ww monitor` prints it, captured the way `ww capture` writes
//! it, and the capture read back the way `ww explain FILE` reads it: the same text.

use super::*;
use crate::util::resolve;
use wire_weaver_client::ww_numeric::NumericValue;
use wire_weaver_client::ww_self::ValueOwned;
use wire_weaver_client::{ClientConfig, DynResource};

mod server {
    use tests_common::TestProcessEvents;
    use wire_weaver::prelude::*;

    pub struct Server {
        pub blob: [u8; 512],
        pub speed: u16,
    }

    impl Server {
        fn add(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
            a: u32,
            b: u32,
        ) -> RpcResult<u32> {
            Ready(a.wrapping_add(b))
        }

        fn blob(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
            len: u16,
        ) -> RpcResult<RefVec<'_, u8>> {
            Ready(RefVec::new_bytes(&self.blob[..(len as usize).min(512)]))
        }

        fn get_speed(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
        ) -> GetResult<u16, ()> {
            Value(self.speed)
        }

        fn set_speed(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
            value: u16,
        ) -> SetResult<()> {
            self.speed = value;
            Set
        }
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            tap_api :: TapDemo for super::Server,
            server = true, no_alloc = true, use_async = false,
            method_model = "_=immediate",
            property_model = "_=get_set",
        );
    }

    impl TestProcessEvents for Server {
        type Medium = ();

        fn process_request_bytes<'a>(
            &mut self,
            bytes: &[u8],
            scratch: &'a mut [u8],
            out: &mut impl BlockingEventOut,
            medium: (),
        ) -> Result<&'a [u8], ShrinkWrapError> {
            self.process_request_bytes(bytes, scratch, out, medium)
        }
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
    let device = tests_common::start_device_usb(
        "cli/tap",
        64,
        server::Server {
            blob: core::array::from_fn(|i| i as u8),
            speed: 7,
        },
        tap_api::TAP_DEMO_FULL_GID,
        server::api_impl::api_hash(),
    );
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
    let device = tests_common::start_device_usb(
        "cli/tap_flags",
        64,
        server::Server {
            blob: [0; 512],
            speed: 7,
        },
        tap_api::TAP_DEMO_FULL_GID,
        server::api_impl::api_hash(),
    );
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

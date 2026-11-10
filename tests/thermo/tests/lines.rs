//! One host driver, devices of three lines (EVO-28): each simulated firmware (`thermo_devices`) connects through the
//! same [Thermo], which picks the client of the device's line in one link setup; every operation behaves the same on
//! each, or fails with an error naming what the device's line lacks.

use thermo_devices as devices;

use thermo::model::{Command, Gain, Mode, Offset, Rate, SelfTestError};
use thermo::{Error, Thermo};
use wire_weaver::ww_version::WireVersion;
use wire_weaver_client::WwClient;

async fn connect(device: &tests_common::TestDevice) -> Thermo {
    Thermo::config(|c| c.in_process_path(device.path()))
        .connect()
        .await
        .expect("a device of a line the driver knows connects")
}

fn gain(hundredths: u16) -> Gain {
    Gain::try_new(hundredths).unwrap()
}

fn rate(hz: u16) -> Rate {
    Rate::try_new(hz).unwrap()
}

fn requests(shared: &devices::Shared) -> u32 {
    shared.lock().unwrap().requests
}

/// Everything [Thermo] does, against a device of any line: `shared` is that device's state.
async fn exercise(t: &Thermo, shared: &devices::Shared) {
    let line = t.wire().line.0;

    // a method, and on line 1 two calls doing its job
    t.configure(rate(50), Mode::Continuous).await.unwrap();
    assert_eq!(shared.lock().unwrap().rate, 50);
    assert_eq!(t.mode().await.unwrap(), Mode::Continuous);
    // a method's error, folded from the line's RateError
    assert!(matches!(
        t.configure(rate(999), Mode::Idle).await,
        Err(Error::Busy)
    ));

    // the same temperature from every line: 0.01 K steps in an i16 on line 1, 0.001 K in an i32 on lines 2 and 3
    let s = t.read_once().await.unwrap();
    assert_eq!(s.temp.raw(), 21_500, "21.5 °C in 0.001 K");
    assert_eq!(s.input.get(), 1);

    // a property; line 1 takes whole steps only and refuses 1.5 before sending anything
    t.set_gain(gain(200)).await.unwrap();
    assert_eq!(t.gain().await.unwrap(), gain(200));
    let before = requests(shared);
    match (line, t.set_gain(gain(150)).await) {
        (1, Err(Error::NotRepresentable { what: "gain", .. })) => {
            assert_eq!(requests(shared), before, "nothing sent")
        }
        (2 | 3, Ok(())) => assert_eq!(t.gain().await.unwrap(), gain(150)),
        (_, r) => panic!("line {line}: {r:?}"),
    }

    // an observed property: the device sends its value when observing starts, then each change it marks; the write
    // may come before or after that first send, so the first value seen is the old gain or the new one
    let mut gains = t.observe_gain().await.unwrap();
    t.set_gain(gain(300)).await.unwrap();
    while gains.recv().await.unwrap() != gain(300) {}

    // a stream up and a sink down: line 1 triggers only in Mode::Triggered, which lines 2 and 3 don't have
    let mut samples = t.samples().await.unwrap();
    let mut commands = t.commands().await.unwrap();
    match line {
        1 => t.set_mode(Mode::Triggered).await.unwrap(),
        _ => assert!(matches!(
            t.set_mode(Mode::Triggered).await,
            Err(Error::Unsupported {
                what: "Mode::Triggered",
                ..
            })
        )),
    }
    commands
        .send(Command::Mark {
            seq: ww_si::Seq::new(100),
        })
        .await
        .unwrap();
    commands.send(Command::Trigger).await.unwrap();
    let s = samples.recv().await.unwrap();
    assert_eq!(s.seq.get(), 100);
    assert_eq!(s.temp.raw(), 21_500);
    // line 3 sends a burst per trigger in one batch, the driver splits it into samples
    if line == 3 {
        for seq in 101..100 + devices::w3::BURST as u16 {
            assert_eq!(samples.recv().await.unwrap().seq.get(), seq);
        }
    }

    // an array of sub-levels
    t.set_channel_enabled(2, false).await.unwrap();
    assert!(!t.channel_enabled(2).await.unwrap());
    assert!(t.channel_enabled(1).await.unwrap());
    assert_eq!(t.channel_last(3).await.unwrap().input.get(), 4);

    // an argument in another resolution: 0.25 K goes to every line, 0.001 K only to lines 2 and 3
    t.calibrate(Offset::new(250)).await.unwrap();
    assert_eq!(t.read_once().await.unwrap().temp.raw(), 21_750);
    let before = requests(shared);
    match (line, t.calibrate(Offset::new(1)).await) {
        (1, Err(Error::NotRepresentable { .. })) => assert_eq!(requests(shared), before),
        (2 | 3, Ok(())) => assert_eq!(t.read_once().await.unwrap().temp.raw(), 21_501),
        (_, r) => panic!("line {line}: {r:?}"),
    }

    // whole seconds on lines 1 and 2, milliseconds in a u64 on line 3 (in a sub-level there)
    assert_eq!(t.uptime().await.unwrap().get(), 42_000);

    // a method lines 2 and 3 have (in a sub-level on line 3)
    match (line, t.self_test().await) {
        (
            1,
            Err(Error::Unsupported {
                what: "self_test",
                needs,
                ..
            }),
        ) => assert_eq!(
            needs, "line 2 from w2.1",
            "from the facade's `#[unsupported]`"
        ),
        (2 | 3, Ok(())) => {
            t.set_channel_enabled(0, false).await.unwrap();
            assert!(matches!(
                t.self_test().await,
                Err(Error::SelfTest(SelfTestError::SensorOpen))
            ));
        }
        (_, r) => panic!("line {line}: {r:?}"),
    }

    // a ww_stdlib trait, the same client and types for every line
    let indication = t.indication();
    indication
        .write_mode(ww_indication::Mode::OnlyEssential)
        .write()
        .await
        .unwrap();
    assert_eq!(
        indication.read_mode().read().await.unwrap(),
        ww_indication::Mode::OnlyEssential
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn line_1_device() {
    let _ = tracing_subscriber::fmt::try_init();
    let (shared, device) = devices::w1::start("thermo/w1", thermo_api_w1::THERMO_WIRE_VERSION);
    let t = connect(&device).await;
    assert_eq!(t.wire(), WireVersion::new(1, 2));
    exercise(&t, &shared).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn line_2_device() {
    let _ = tracing_subscriber::fmt::try_init();
    let (shared, device) = devices::w2::start("thermo/w2", thermo_api_w2::THERMO_WIRE_VERSION);
    let t = connect(&device).await;
    assert_eq!(t.wire(), WireVersion::new(2, 1));
    exercise(&t, &shared).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn line_3_device() {
    let _ = tracing_subscriber::fmt::try_init();
    let (shared, device) = devices::w3::start("thermo/w3", thermo_api_w3::THERMO_WIRE_VERSION);
    let t = connect(&device).await;
    assert_eq!(t.wire(), WireVersion::new(3, 0));
    exercise(&t, &shared).await;
}

/// Firmware of the first revision of each line, `thermo_api` 1.0.0 at w1.0 and 2.0.0 at w2.0, which the driver has no
/// client of: it talks to them with the client of their line, and items with a newer `#[since]` are
/// [Error::Unsupported], naming the revision they need, never reaching the device.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn older_revisions() {
    let _ = tracing_subscriber::fmt::try_init();
    let (shared, device) = devices::w1_0::start("thermo/w1_0");
    let t = connect(&device).await;
    let before = requests(&shared);
    let e = t.calibrate(Offset::new(250)).await.unwrap_err();
    assert!(
        matches!(&e, Error::Unsupported { what: "calibrate", needs, .. } if needs == "w1.1"),
        "{e:?}"
    );
    let e = t.uptime().await.unwrap_err();
    assert!(
        matches!(&e, Error::Unsupported { what: "uptime", needs, .. } if needs == "w1.2"),
        "{e:?}"
    );
    assert_eq!(
        e.to_string(),
        "uptime is not on this device: it is at wire w1.0, uptime needs w1.2"
    );
    assert_eq!(requests(&shared), before, "nothing sent");
    t.read_once().await.unwrap();

    let (shared, device) = devices::w2_0::start("thermo/w2_0");
    let t = connect(&device).await;
    let before = requests(&shared);
    let e = t.self_test().await.unwrap_err();
    assert!(
        matches!(&e, Error::Unsupported { what: "self_test", needs, .. } if needs == "w2.1"),
        "{e:?}"
    );
    assert_eq!(requests(&shared), before, "nothing sent");
    t.uptime().await.unwrap();
}

/// A device on a line the driver has no client of is refused in link setup, the error naming every line the host
/// speaks and who should update.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_fourth_line_is_refused() {
    let _ = tracing_subscriber::fmt::try_init();
    let (shared, device) = devices::w3::start("thermo/w4", WireVersion::new(4, 0));
    let e = Thermo::config(|c| c.in_process_path(device.path()))
        .connect()
        .await
        .err()
        .expect("no client of line 4");
    let Some(wire_weaver_client::Error::ConnectFailed { reason, .. }) =
        e.downcast_ref::<wire_weaver_client::Error>()
    else {
        panic!("{e:?}");
    };
    assert!(
        reason.contains("is at wire w4.0")
            && reason.contains("this host's clients at wires w3.0, w2.1, w1.2")
            && reason.contains("update the host to a client of line 4"),
        "{reason}"
    );
    assert_eq!(requests(&shared), 0);
}

/// CLT-63: a DFU brings the device back on another wire line the facade also speaks. The plain
/// `Commander::reconnect` refuses it, naming both lines; `Thermo::reconnect` (`Commander::reconnect_as`) picks
/// the new line's client instead.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnect_after_a_dfu_to_another_line() {
    let _ = tracing_subscriber::fmt::try_init();
    let path = "thermo/dfu";
    let (_shared1, device1) = devices::w1::start(path, thermo_api_w1::THERMO_WIRE_VERSION);
    let t = connect(&device1).await;
    assert_eq!(t.wire(), WireVersion::new(1, 2));
    t.read_once().await.unwrap();

    t.cmd().disconnect().keep_streams().await.unwrap();
    // the device at `path` comes back flashed with line 3's firmware
    let (shared3, _device3) = devices::w3::start(path, thermo_api_w3::THERMO_WIRE_VERSION);

    let e = t.cmd().reconnect().await.unwrap_err();
    assert!(
        matches!(
            &e,
            wire_weaver_client::Error::ReconnectedOnAnotherLine { old_wire, new_wire, .. }
                if *old_wire == WireVersion::new(1, 2) && *new_wire == WireVersion::new(3, 0)
        ),
        "{e:?}"
    );
    let text = e.to_string();
    assert!(
        text.contains("w3.0") && text.contains("w1.2"),
        "both lines named: {text}"
    );

    // `reconnect` left the Commander disconnected again: a fresh reconnect_as picks line 3's client
    let t3 = t.reconnect().await.expect("the facade also speaks line 3");
    assert_eq!(t3.wire(), WireVersion::new(3, 0));
    let before = requests(&shared3);
    let s = t3.read_once().await.unwrap();
    assert_eq!(s.temp.raw(), 21_500);
    assert!(requests(&shared3) > before, "t3 talks to the w3 device");
}

/// [Thermo::default_config] speaks three lines, and they are three lines: a host speaking one line twice, or another
/// API, is refused when connecting (`wire_weaver_client`'s `host_of_two_lines_*` tests).
#[test]
fn the_driver_speaks_three_lines() {
    const {
        let (w1, w2, w3) = (
            thermo_api_w1::THERMO_WIRE_VERSION.line.0,
            thermo_api_w2::THERMO_WIRE_VERSION.line.0,
            thermo_api_w3::THERMO_WIRE_VERSION.line.0,
        );
        assert!(w1 != w2 && w2 != w3 && w1 != w3)
    };
    let _ = Thermo::default_config();
}

/// `ww api check` agrees on both breaks: line 2 against line 1 and line 3 against line 2 each need a new line and a
/// breaking crate bump, and the versions chosen have both. Each line's later revision needs a new revision and a
/// minor bump. Across a break resources are matched by path and `#[moved_from]` (EVO-40), within a line by position.
#[test]
fn checker_agrees_on_both_bumps() {
    use wire_weaver_evolution::{
        Change, Move, WireBump, compare, compare_moved, snapshot_files::triplet,
    };
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../thermo_api");
    let load = |v: &str| wire_weaver_core::load_crate(&dir.join(v)).unwrap();
    let (w1_0, w1, w2_0, w2) = (load("w1_0"), load("w1"), load("w2_0"), load("w2"));

    let report = compare(&w1, &w2_0).unwrap();
    assert!(report.across_line);
    assert_eq!(report.change, Change::Breaking);
    assert_eq!(report.wire_bump, WireBump::Line);
    assert_eq!(report.minimal_wire(), Some(WireVersion::new(2, 0)));
    assert_eq!(triplet(&report.minimal_version()), "2.0.0");
    report.check_version().unwrap();
    report.check_wire().unwrap();
    // what the wire break is made of, the resources matched by path: `configure` replaces `set_rate`, not a rename
    // of it with another argument
    let reasons = report.wire_reasons.join("\n");
    for broke in [
        "type thermo_api::Sample changed",
        "Thermo::set_rate removed",
        "Thermo::gain: B of Fixed",
        "Thermo::calibrate: argument `offset`: P of Fixed",
    ] {
        assert!(reasons.contains(broke), "{broke} not in:\n{reasons}");
    }
    assert!(
        report
            .compatible
            .contains(&"Thermo::configure added".to_string()),
        "{:?}",
        report.compatible
    );
    // the root's own `Channel` is not a break of its own, and the new side is not labelled with the old version
    assert!(
        !reasons.contains("trait thermo_api::Channel changed"),
        "{reasons}"
    );
    assert!(!reasons.contains("different wire lines"), "{reasons}");
    // a field that changed scale and storage reports both (EVO-33)
    let sample = report
        .breaking
        .iter()
        .find(|f| f.message.starts_with("struct Sample: field `temp`"))
        .expect("Sample::temp");
    for both in [
        "prefix::Centi",
        "argument 2 of Fixed: i16 on the old version, i32 on the new version",
    ] {
        assert!(
            sample.message.contains(both),
            "{both} not in {}",
            sample.message
        );
    }
    // Rust-only: the variant gone; nothing is a rename across a break
    let breaking: Vec<_> = report.breaking.iter().map(|f| f.message.as_str()).collect();
    assert!(
        breaking
            .iter()
            .any(|m| m.contains("Mode::Triggered removed")),
        "{breaking:#?}"
    );
    assert!(
        !breaking.iter().any(|m| m.contains("renamed")),
        "{breaking:#?}"
    );

    // line 2 -> line 3: the batched stream, the tree moved, uptime in another unit and type
    let (w3, notes) = wire_weaver_core::load_crate_with_notes(&dir.join("w3")).unwrap();
    let moves: Vec<Move> = notes.moved_from.iter().map(Move::from).collect();
    assert_eq!(moves.len(), 1, "{moves:?}");
    let report = compare_moved(&w2, &w3, &moves).unwrap();
    assert_eq!(report.change, Change::Breaking);
    assert_eq!(report.wire_bump, WireBump::Line);
    assert_eq!(report.minimal_wire(), Some(WireVersion::new(3, 0)));
    assert_eq!(triplet(&report.minimal_version()), "3.0.0");
    report.check_version().unwrap();
    report.check_wire().unwrap();
    // the batch replaces `Sample` without a field-by-field comparison, `self_test` moved into `health` by its
    // `#[moved_from]`, `uptime` removed, its namesake in `health` only proposed, with the reason it can't be the same
    let reasons = report.wire_reasons.join("\n");
    for broke in [
        "Thermo::samples: Batch on the new version, Sample on the old version: another type",
        "Thermo::self_test moved to Health::self_test",
        "Thermo::uptime removed",
    ] {
        assert!(reasons.contains(broke), "{broke} not in:\n{reasons}");
    }
    assert!(
        !reasons.contains("type thermo_api::Sample changed"),
        "{reasons}"
    );
    assert!(!reasons.contains("Thermo::self_test removed"), "{reasons}");
    assert_eq!(report.proposals.len(), 1, "{:#?}", report.proposals);
    assert!(
        report.proposals[0].starts_with(
            "Health::uptime is named like Thermo::uptime of w2.1, removed, but its \
             signature differs"
        ),
        "{}",
        report.proposals[0]
    );

    for (old, new, wire, crate_version) in
        [(&w1_0, &w1, (1, 1), "1.1.0"), (&w2_0, &w2, (2, 1), "2.1.0")]
    {
        let report = compare(old, new).unwrap();
        assert_eq!(report.change, Change::Compatible, "{:?}", report.breaking);
        assert_eq!(report.wire_bump, WireBump::Revision);
        assert_eq!(
            report.minimal_wire(),
            Some(WireVersion::new(wire.0, wire.1))
        );
        assert_eq!(triplet(&report.minimal_version()), crate_version);
        report.check_version().unwrap();
        report.check_wire().unwrap();
    }
}

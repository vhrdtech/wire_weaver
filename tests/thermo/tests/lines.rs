//! One host driver, devices of both lines (EVO-28): each simulated firmware connects through the same [Thermo], which
//! picks the client of the device's line in one link setup; every operation behaves the same on both, or fails with
//! an error naming what the device's line lacks.

mod devices;

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

/// Everything [Thermo] does, against a device of either line: `shared` is that device's state.
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

    // the same temperature from both lines: 0.01 K steps in an i16 on line 1, 0.001 K in an i32 on line 2
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
        (2, Ok(())) => assert_eq!(t.gain().await.unwrap(), gain(150)),
        (_, r) => panic!("line {line}: {r:?}"),
    }

    // an observed property: the device sends its value when observing starts, then each change it marks; the write
    // may come before or after that first send, so the first value seen is the old gain or the new one
    let mut gains = t.observe_gain().await.unwrap();
    t.set_gain(gain(300)).await.unwrap();
    while gains.recv().await.unwrap() != gain(300) {}

    // a stream up and a sink down: line 1 triggers only in Mode::Triggered, which line 2 doesn't have
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

    // an array of sub-levels
    t.set_channel_enabled(2, false).await.unwrap();
    assert!(!t.channel_enabled(2).await.unwrap());
    assert!(t.channel_enabled(1).await.unwrap());
    assert_eq!(t.channel_last(3).await.unwrap().input.get(), 4);

    // an argument in another resolution: 0.25 K goes to both, 0.001 K only to line 2
    t.calibrate(Offset::new(250)).await.unwrap();
    assert_eq!(t.read_once().await.unwrap().temp.raw(), 21_750);
    let before = requests(shared);
    match (line, t.calibrate(Offset::new(1)).await) {
        (1, Err(Error::NotRepresentable { .. })) => assert_eq!(requests(shared), before),
        (2, Ok(())) => assert_eq!(t.read_once().await.unwrap().temp.raw(), 21_501),
        (_, r) => panic!("line {line}: {r:?}"),
    }

    assert_eq!(t.uptime().await.unwrap().get(), 42);

    // a method only line 2 has
    match (line, t.self_test().await) {
        (
            1,
            Err(Error::Unsupported {
                what: "self_test", ..
            }),
        ) => {}
        (2, Ok(())) => {
            t.set_channel_enabled(0, false).await.unwrap();
            assert!(matches!(
                t.self_test().await,
                Err(Error::SelfTest(SelfTestError::SensorOpen))
            ));
        }
        (_, r) => panic!("line {line}: {r:?}"),
    }

    // a ww_stdlib trait, the same client and types for both lines
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
async fn a_third_line_is_refused() {
    let _ = tracing_subscriber::fmt::try_init();
    let (shared, device) = devices::w2::start("thermo/w3", WireVersion::new(3, 0));
    let e = Thermo::config(|c| c.in_process_path(device.path()))
        .connect()
        .await
        .err()
        .expect("no client of line 3");
    let Some(wire_weaver_client::Error::ConnectFailed { reason, .. }) =
        e.downcast_ref::<wire_weaver_client::Error>()
    else {
        panic!("{e:?}");
    };
    assert!(
        reason.contains("is at wire w3.0")
            && reason.contains("this host's clients at wires w2.1, w1.2")
            && reason.contains("update the host to a client of line 3"),
        "{reason}"
    );
    assert_eq!(requests(&shared), 0);
}

/// [Thermo::default_config] speaks both lines, and they are two lines: a host speaking one line twice, or another
/// API, is refused when connecting (`wire_weaver_client`'s `host_of_two_lines_*` tests).
#[test]
fn the_driver_speaks_both_lines() {
    const {
        assert!(
            thermo_api_w1::THERMO_WIRE_VERSION.line.0 != thermo_api_w2::THERMO_WIRE_VERSION.line.0
        )
    };
    let _ = Thermo::default_config();
}

/// `ww api check` agrees on the break: line 2 against line 1 needs a new line and a breaking crate bump, and the
/// versions chosen have both. Each line's later revision needs a new revision and a minor bump.
#[test]
fn checker_agrees_on_both_bumps() {
    use wire_weaver_evolution::{Change, WireBump, compare, snapshot_files::triplet};
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../thermo_api");
    let load = |v: &str| wire_weaver_core::load_crate(&dir.join(v)).unwrap();
    let (w1_0, w1, w2_0, w2) = (load("w1_0"), load("w1"), load("w2_0"), load("w2"));

    let report = compare(&w1, &w2_0).unwrap();
    assert_eq!(report.change, Change::Breaking);
    assert_eq!(report.wire_bump, WireBump::Line);
    assert_eq!(report.minimal_wire(), Some(WireVersion::new(2, 0)));
    assert_eq!(triplet(&report.minimal_version()), "2.0.0");
    report.check_version().unwrap();
    report.check_wire().unwrap();
    // what the wire break is made of, the resources matched by position: `configure` is `set_rate` renamed, with
    // another argument
    let reasons = report.wire_reasons.join("\n");
    for broke in [
        "type thermo_api::Sample changed",
        "Thermo::set_rate: argument `config`",
        "Thermo::gain: B of Fixed",
        "Thermo::calibrate: argument `offset`: P of Fixed",
    ] {
        assert!(reasons.contains(broke), "{broke} not in:\n{reasons}");
    }
    // Rust-only: the rename, the variant gone
    let breaking: Vec<_> = report.breaking.iter().map(|f| f.message.as_str()).collect();
    for rust in [
        "Thermo::set_rate renamed to configure",
        "Mode::Triggered removed",
    ] {
        assert!(
            breaking.iter().any(|m| m.contains(rust)),
            "{rust} not in {breaking:#?}"
        );
    }

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

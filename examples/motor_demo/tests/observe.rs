//! The demo device observed through the generated client: each tier at its own rate, the budget, sim time faster
//! and slower than real time, UDP.

use std::collections::BTreeMap;
use std::time::Duration;

use motor_demo::client::MotorDemo;
use motor_demo::device::{self, Medium, Options};
use motor_demo::host::{LevelRequest, observe};
use motor_demo::stats::{Level, Stats, find};
use wire_weaver_client::observe::{on_change, periodic};
use ww_client_server::observe::ObserveMode;

const MS: Duration = Duration::from_millis(1);

fn modes(names: &[&str], mode: ObserveMode) -> BTreeMap<u32, ObserveMode> {
    names
        .iter()
        .map(|n| (find(n).unwrap().index, mode))
        .collect()
}

fn all(level: Level, mode: ObserveMode) -> BTreeMap<u32, ObserveMode> {
    level.props().map(|p| (p.index, mode)).collect()
}

async fn connect_in_process(path: &str, opts: Options) -> (device::DeviceThread, MotorDemo) {
    let dev = device::spawn(Medium::InProcess(path.into()), opts).unwrap();
    let client = MotorDemo::config(|c| c.in_process_path(path.to_string()))
        .connect()
        .await
        .unwrap();
    (dev, client)
}

/// Count for `real` seconds after a short settling time.
async fn run(session: &motor_demo::host::Session, real: Duration) -> Stats {
    tokio::time::sleep(100 * MS).await;
    session.take();
    tokio::time::sleep(real).await;
    session.take()
}

fn rate(stats: &Stats, name: &str) -> f64 {
    let p = find(name).unwrap();
    let count = stats.levels[&p.level]
        .props
        .get(&p.index)
        .map_or(0, |s| s.count);
    count as f64 / stats.since.elapsed().as_secs_f64()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_tier_at_its_rate() {
    let (_dev, client) = connect_in_process("motor_demo/tiers", Options::default()).await;
    let session = observe(
        &client,
        &[
            LevelRequest {
                level: Level::Foc,
                modes: modes(&["i_d", "i_q", "phase"], periodic(MS)),
                budget: Some(100_000),
            },
            LevelRequest {
                level: Level::Control,
                modes: all(Level::Control, periodic(10 * MS)),
                budget: Some(100_000),
            },
            LevelRequest {
                level: Level::Telemetry,
                // changes 50 times per second, sent each time
                modes: modes(&["time_s", "watt_hours"], on_change(MS)),
                budget: None,
            },
        ],
    )
    .await
    .unwrap();
    assert_eq!(session.granted[&Level::Foc].budget, 100_000);
    assert_eq!(
        session.granted[&Level::Telemetry].budget,
        device::LIMITS.default_budget
    );
    let stats = run(&session, Duration::from_secs(1)).await;
    let near = |name, hz: f64| {
        let r = rate(&stats, name);
        assert!(
            (r - hz).abs() < 0.15 * hz,
            "{name}: {r:.1} Hz, expected {hz}"
        );
    };
    near("i_q", 1_000.0);
    near("phase", 1_000.0);
    near("erpm", 100.0);
    near("torque", 100.0);
    near("time_s", 50.0);
    assert_eq!(rate(&stats, "i_a"), 0.0);
    assert_eq!(stats.levels[&Level::Foc].missed, 0);
    // 18 values in one update instead of 18 events
    let control = &stats.levels[&Level::Control];
    assert!(
        (control.bytes as f64) < 0.6 * control.stream_data_bytes() as f64,
        "{control:?}"
    );
}

/// A budget below what the asked rates take: the device sends what fits and counts the rest as missed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn budget_limits_the_rate() {
    let (_dev, client) = connect_in_process("motor_demo/budget", Options::default()).await;
    let session = observe(
        &client,
        &[LevelRequest {
            level: Level::Foc,
            modes: all(Level::Foc, periodic(MS)),
            budget: Some(20_000),
        }],
    )
    .await
    .unwrap();
    let stats = run(&session, Duration::from_secs(1)).await;
    let foc = &stats.levels[&Level::Foc];
    let bytes_per_s = foc.bytes as f64 / stats.since.elapsed().as_secs_f64();
    // 21 f32 a millisecond would be ~90 kB/s
    assert!(bytes_per_s < 22_000.0, "{bytes_per_s:.0} B/s");
    assert!(bytes_per_s > 15_000.0, "{bytes_per_s:.0} B/s");
    assert!(foc.missed > 0, "{foc:?}");
}

/// Simulated time ten times slower: periods are in device time, so a 1 ms period arrives at 100 Hz real; the 25 kHz
/// tier can be watched at every PWM period.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn slow_motion() {
    let opts = Options {
        time_scale: 0.1,
        ..Options::default()
    };
    let (_dev, client) = connect_in_process("motor_demo/slow", opts).await;
    let session = observe(
        &client,
        &[LevelRequest {
            level: Level::Foc,
            modes: modes(&["i_q", "i_d"], periodic(Duration::from_micros(40))),
            budget: Some(1_000_000),
        }],
    )
    .await
    .unwrap();
    let stats = run(&session, Duration::from_secs(1)).await;
    // 25 kHz of device time at a tenth of real time
    let r = rate(&stats, "i_q");
    assert!((r - 2_500.0).abs() < 400.0, "{r:.0} Hz");
    let sim = stats.levels[&Level::Foc].sim_span_s();
    assert!((sim - 0.1).abs() < 0.02, "{sim} s of device time");
}

/// Faster than real time: the device's clock runs ahead, a 10 ms period arrives five times as often.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fast_forward() {
    let opts = Options {
        time_scale: 5.0,
        ..Options::default()
    };
    let (_dev, client) = connect_in_process("motor_demo/fast", opts).await;
    let session = observe(
        &client,
        &[LevelRequest {
            level: Level::Control,
            modes: modes(&["erpm"], periodic(10 * MS)),
            budget: None,
        }],
    )
    .await
    .unwrap();
    let stats = run(&session, Duration::from_secs(1)).await;
    let r = rate(&stats, "erpm");
    assert!((r - 500.0).abs() < 75.0, "{r:.0} Hz");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn over_udp() {
    let dev = device::spawn(
        Medium::Udp("127.0.0.1:0".parse().unwrap()),
        Options::default(),
    )
    .unwrap();
    let addr = dev.udp_addr.unwrap();
    let client = MotorDemo::config(|c| c.udp_addr(addr.to_string()))
        .connect()
        .await
        .unwrap();
    let session = observe(
        &client,
        &[
            LevelRequest {
                level: Level::Foc,
                modes: all(Level::Foc, periodic(5 * MS)),
                budget: Some(50_000),
            },
            LevelRequest {
                level: Level::Control,
                modes: all(Level::Control, periodic(10 * MS)),
                budget: Some(50_000),
            },
        ],
    )
    .await
    .unwrap();
    let stats = run(&session, Duration::from_secs(1)).await;
    let r = rate(&stats, "v_bus");
    assert!((r - 200.0).abs() < 30.0, "{r:.0} Hz");
    let r = rate(&stats, "rpm");
    assert!((r - 100.0).abs() < 15.0, "{r:.0} Hz");
    // the profile runs: the speed moves
    let erpm: f32 = stats.levels[&Level::Control].props[&5]
        .last
        .parse()
        .unwrap();
    assert!(erpm.abs() > 1_000.0, "{erpm}");
}

//! The thermometer as a host sees it: the simulated device served over the in-process WebSocket
//! (`wire_weaver_client::in_process::ws_device`), on tokio's paused clock, so time moves only when both ends wait.

use std::time::Duration;

use thermometer::{Counters, SimBoard, Thermometer};
use thermometer_api::{Command, Gain, Mode, Rate, RateError};
use thermometer_device::Input;
use tokio::sync::oneshot;
use wire_weaver_client::observe::on_change;
use wire_weaver_client::{CallError, in_process};

/// Runs `test` against a simulated thermometer at `path`; returns the device's counters after it stopped.
async fn with_device<F: Future<Output = ()>>(
    path: &str,
    test: impl FnOnce(Thermometer) -> F,
) -> Counters {
    let local = tokio::task::LocalSet::new();
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let incoming = in_process::ws_device(path);
    let device = local.spawn_local(thermometer::simulated(incoming, async {
        _ = stop_rx.await;
    }));
    local
        .run_until(async {
            let host = Thermometer::config(|c| c.in_process_path(path.to_owned()).no_ww_usb())
                .connect()
                .await
                .expect("connects");
            test(host).await;
            _ = stop_tx.send(());
            device.await.expect("the device runs to its stop")
        })
        .await
}

#[tokio::test(start_paused = true)]
async fn read_once_reads_input_1() {
    with_device("thermometer/read_once", |t| async move {
        let sample = t.read_once().call().await.unwrap();
        assert_eq!(sample.input.get(), 1);
        assert_eq!(sample.seq.get(), 0);
        // the room is at 21.50 °C give or take 1 °C
        assert!((2050..=2250).contains(&sample.temp.raw()), "{sample:?}");
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn continuous_samples_every_enabled_input_at_the_rate() {
    let counters = with_device("thermometer/continuous", |t| async move {
        let mut samples = t.samples().call().await.unwrap();
        t.channel(1).write_enabled(false).write().await.unwrap();
        t.set_rate(Rate::bounded(5)).call().await.unwrap();
        t.write_mode(Mode::Continuous).write().await.unwrap();
        let started = tokio::time::Instant::now();
        let mut got = vec![];
        for _ in 0..9 {
            got.push(samples.recv().await.unwrap());
        }
        // three ticks of 200 ms, the first at once
        assert_eq!(started.elapsed(), Duration::from_millis(400));
        t.write_mode(Mode::Idle).write().await.unwrap();
        let inputs: Vec<u8> = got.iter().map(|s| s.input.get()).collect();
        assert_eq!(inputs, [1, 3, 4, 1, 3, 4, 1, 3, 4]);
        let seqs: Vec<u16> = got.iter().map(|s| s.seq.get()).collect();
        assert_eq!(seqs, [0, 1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(!t.channel(1).read_enabled().read().await.unwrap());
    })
    .await;
    assert_eq!(counters.samples_missed, 0);
}

#[tokio::test(start_paused = true)]
async fn a_triggered_conversion_makes_set_rate_busy() {
    let counters = with_device("thermometer/triggered", |t| async move {
        let mut samples = t.samples().call().await.unwrap();
        let mut commands = t.commands().await.unwrap();
        // outside Triggered a trigger does nothing
        commands.send(Command::Trigger).await.unwrap();
        t.write_mode(Mode::Triggered).write().await.unwrap();
        commands.send(Command::Trigger).await.unwrap();
        // the board's conversion takes 100 ms, in which the rate can't change
        let busy = t.set_rate(Rate::bounded(20)).call().await;
        assert!(
            matches!(busy, Err(CallError::Device(RateError::Busy))),
            "{busy:?}"
        );
        let sample = samples.recv().await.unwrap();
        assert_eq!(sample.input.get(), 1);
        t.set_rate(Rate::bounded(20)).call().await.unwrap();
    })
    .await;
    assert_eq!(counters.triggers_ignored, 1);
}

#[tokio::test(start_paused = true)]
async fn mark_restarts_the_numbering() {
    with_device("thermometer/mark", |t| async move {
        let mut commands = t.commands().await.unwrap();
        commands
            .send(Command::Mark {
                seq: ww_si::Seq::new(1000),
            })
            .await
            .unwrap();
        // the sink is in order with the calls after it on the same link
        let sample = t.read_once().call().await.unwrap();
        assert_eq!(sample.seq.get(), 1000);
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn the_device_lowers_the_gain_and_observers_see_it() {
    with_device("thermometer/gain", |t| async move {
        let mut gain = t
            .observe_gain(on_change(Duration::ZERO))
            .subscribe()
            .await
            .unwrap();
        assert_eq!(gain.recv().await.unwrap().value.get(), 4);
        t.write_gain(Gain::bounded(16)).write().await.unwrap();
        assert_eq!(gain.recv().await.unwrap().value.get(), 16);
        // above gain 8 the simulated amplifier clips: the reading halves the gain
        t.read_once().call().await.unwrap();
        assert_eq!(gain.recv().await.unwrap().value.get(), 8);
        assert_eq!(t.read_gain().read().await.unwrap().get(), 8);
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn the_board_has_four_channels() {
    with_device("thermometer/channels", |t| async move {
        let valid = t.channel_valid_indices().read().await.unwrap();
        assert!(valid.contains(3) && !valid.contains(4), "{valid:?}");
        let last = t.channel(3).last().call().await.unwrap();
        assert_eq!(last.input.get(), 4);
        assert!(t.channel(4).last().call().await.is_err());
    })
    .await;
}

#[test]
fn the_simulated_room_drifts_once_a_minute() {
    let at = |s: u64| {
        SimBoard::temperature(Input::bounded(1), ww_device::Instant::from_millis(s * 1000)).raw()
    };
    assert_eq!(at(0), 2050);
    assert_eq!(at(30), 2250);
    assert_eq!(at(60), 2050);
    assert_eq!(
        SimBoard::temperature(Input::bounded(4), ww_device::Instant::from_micros(0)).raw(),
        2125
    );
}

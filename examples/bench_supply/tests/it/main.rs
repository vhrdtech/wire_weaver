//! The bench supply as a host sees it: the simulated device served over the in-process WebSocket
//! (`wire_weaver_client::in_process::ws_device`), on tokio's paused clock. The `*_example` modules are the examples
//! whose output the book shows, run here against the simulated device; their `main` only runs from `cargo run`.

mod book;

#[path = "../../examples/errors.rs"]
#[expect(dead_code, reason = "main runs from cargo run, not here")]
mod errors_example;

use bench_supply::BenchSupply;
use bench_supply_api::OutputError;
use tokio::sync::oneshot;
use wire_weaver_client::observe::on_change;
use wire_weaver_client::{CallError, in_process};

/// Runs `test` against a simulated bench supply at `path`.
async fn with_device<F: Future<Output = ()>>(path: &str, test: impl FnOnce(BenchSupply) -> F) {
    let local = tokio::task::LocalSet::new();
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let incoming = in_process::ws_device(path);
    let device = local.spawn_local(bench_supply::simulated(incoming, async {
        _ = stop_rx.await;
    }));
    local
        .run_until(async {
            let host = BenchSupply::config(|c| c.in_process_path(path.to_owned()).no_ww_usb())
                .connect()
                .await
                .expect("connects");
            test(host).await;
            _ = stop_tx.send(());
            device.await.expect("the device runs to its stop")
        })
        .await;
}

#[tokio::test(start_paused = true)]
async fn output_refuses_with_no_voltage_set() {
    with_device("bench_supply/no_voltage", |b| async move {
        let err = b.channel(0).set_output(true).call().await;
        assert!(
            matches!(err, Err(CallError::Device(OutputError::NoVoltageSet))),
            "{err:?}"
        );
        assert!(!b.channel(0).read_output_on().read().await.unwrap());
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn output_refuses_over_current_and_says_how_much() {
    with_device("bench_supply/over_current", |b| async move {
        // channel 0's load is 10 Ω: at 30 V it would draw 3 A
        b.channel(0)
            .write_voltage_limit(bench_supply_api::Voltage::bounded(30_000))
            .write()
            .await
            .unwrap();
        b.channel(0)
            .write_current_limit(bench_supply_api::Current::bounded(1_000))
            .write()
            .await
            .unwrap();
        let err = b.channel(0).set_output(true).call().await;
        match err {
            Err(CallError::Device(OutputError::OverCurrent { would_draw })) => {
                assert_eq!(would_draw.get(), 3_000);
            }
            other => panic!("{other:?}"),
        }
        assert!(!b.channel(0).read_output_on().read().await.unwrap());
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn output_turns_on_within_limits_and_measures() {
    with_device("bench_supply/within_limits", |b| async move {
        b.channel(0)
            .write_voltage_limit(bench_supply_api::Voltage::bounded(5_000))
            .write()
            .await
            .unwrap();
        b.channel(0)
            .write_current_limit(bench_supply_api::Current::bounded(1_000))
            .write()
            .await
            .unwrap();
        b.channel(0).set_output(true).call().await.unwrap();
        assert!(b.channel(0).read_output_on().read().await.unwrap());
        let measured = b.channel(0).read_measured().read().await.unwrap();
        assert_eq!(measured.voltage.get(), 5_000);
        // 5 V over the channel's 10 Ω load is 500 mA
        assert_eq!(measured.current.get(), 500);

        b.channel(0).set_output(false).call().await.unwrap();
        assert!(!b.channel(0).read_output_on().read().await.unwrap());
        let measured = b.channel(0).read_measured().read().await.unwrap();
        assert_eq!(measured.voltage.get(), 0);
        assert_eq!(measured.current.get(), 0);
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn channels_are_independent() {
    with_device("bench_supply/independent", |b| async move {
        b.channel(0)
            .write_voltage_limit(bench_supply_api::Voltage::bounded(5_000))
            .write()
            .await
            .unwrap();
        b.channel(0)
            .write_current_limit(bench_supply_api::Current::bounded(1_000))
            .write()
            .await
            .unwrap();
        b.channel(0).set_output(true).call().await.unwrap();

        assert!(b.channel(0).read_output_on().read().await.unwrap());
        assert!(!b.channel(1).read_output_on().read().await.unwrap());
        assert_eq!(
            b.channel(1)
                .read_voltage_limit()
                .read()
                .await
                .unwrap()
                .get(),
            0
        );
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn observers_see_output_on_change() {
    with_device("bench_supply/observe", |b| async move {
        let mut output_on = b
            .channel(0)
            .observe_output_on(on_change(std::time::Duration::ZERO))
            .subscribe()
            .await
            .unwrap();
        assert!(!output_on.recv().await.unwrap().value);
        b.channel(0)
            .write_voltage_limit(bench_supply_api::Voltage::bounded(1_000))
            .write()
            .await
            .unwrap();
        b.channel(0)
            .write_current_limit(bench_supply_api::Current::bounded(1_000))
            .write()
            .await
            .unwrap();
        b.channel(0).set_output(true).call().await.unwrap();
        assert!(output_on.recv().await.unwrap().value);
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn limits_refuse_a_write_while_the_output_is_on() {
    with_device("bench_supply/wrong_state", |b| async move {
        let channel = b.channel(0);
        channel
            .write_voltage_limit(bench_supply_api::Voltage::bounded(5_000))
            .write()
            .await
            .unwrap();
        channel
            .write_current_limit(bench_supply_api::Current::bounded(1_000))
            .write()
            .await
            .unwrap();
        channel.set_output(true).call().await.unwrap();
        for refused in [
            channel
                .write_voltage_limit(bench_supply_api::Voltage::bounded(6_000))
                .write()
                .await,
            channel
                .write_current_limit(bench_supply_api::Current::bounded(2_000))
                .write()
                .await,
        ] {
            let err = refused.expect_err("refused while on");
            assert!(
                matches!(
                    err.common(),
                    Some(wire_weaver::prelude::CommonErrorOwned::WrongState { .. })
                ),
                "{err:?}"
            );
        }
        assert_eq!(
            channel.read_voltage_limit().read().await.unwrap().get(),
            5_000
        );
        channel.set_output(false).call().await.unwrap();
        channel
            .write_voltage_limit(bench_supply_api::Voltage::bounded(6_000))
            .write()
            .await
            .expect("off: the limit may change");
    })
    .await;
}

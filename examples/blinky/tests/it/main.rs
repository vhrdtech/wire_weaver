//! The blinky device as a host sees it: the simulated device served over the in-process WebSocket
//! (`wire_weaver_client::in_process::ws_device`), on tokio's paused clock.

mod book;
// The host program the book runs, compiled into the tests so that `book::book_output_host` calls its `run`.
#[path = "../../examples/host.rs"]
#[expect(dead_code, reason = "the example's own main and arguments; the tests call only its run")]
mod host;

use std::cell::RefCell;
use std::rc::Rc;

use blinky::{Blinky, Led};
use tokio::sync::oneshot;
use wire_weaver_client::in_process;

/// An LED that records every state it is set to.
struct RecordedLed(Rc<RefCell<Vec<bool>>>);

impl Led for RecordedLed {
    fn set(&mut self, on: bool) {
        self.0.borrow_mut().push(on);
    }
}

/// Runs `test` against a simulated blinky device at `path`; returns the states its LED was set to.
async fn with_device<F: Future<Output = ()>>(
    path: &str,
    test: impl FnOnce(Blinky) -> F,
) -> Vec<bool> {
    let local = tokio::task::LocalSet::new();
    let states = Rc::new(RefCell::new(vec![]));
    let led: &'static mut RecordedLed = Box::leak(Box::new(RecordedLed(states.clone())));
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let device = local.spawn_local(blinky::simulated(in_process::ws_device(path), led, async {
        _ = stop_rx.await;
    }));
    local
        .run_until(async {
            let host = Blinky::config(|c| c.in_process_path(path.to_owned()).no_ww_usb())
                .connect()
                .await
                .expect("connects");
            test(host).await;
            _ = stop_tx.send(());
            device.await.expect("the device runs to its stop");
        })
        .await;
    states.take()
}

#[tokio::test(start_paused = true)]
async fn led_on_and_off_reach_the_led() {
    let states = with_device("blinky/on_off", |b| async move {
        b.led_on().call().await.unwrap();
        b.led_off().call().await.unwrap();
    })
    .await;
    // off at start, then on and off
    assert_eq!(states, [false, true, false]);
}

#[tokio::test(start_paused = true)]
async fn set_led_returns_the_state_before() {
    with_device("blinky/set_led", |b| async move {
        assert!(!b.set_led(true).call().await.unwrap());
        assert!(b.set_led(true).call().await.unwrap());
        assert!(b.set_led(false).call().await.unwrap());
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn blinks_counts_each_time_the_led_goes_on() {
    with_device("blinky/blinks", |b| async move {
        assert_eq!(b.blinks().call().await.unwrap().get(), 0);
        b.led_on().call().await.unwrap();
        // already on: no new blink
        b.set_led(true).call().await.unwrap();
        b.led_off().call().await.unwrap();
        b.led_on().call().await.unwrap();
        assert_eq!(b.blinks().call().await.unwrap().get(), 2);
    })
    .await;
}

//! Keep-trying connections end to end (CLT-75): a generated client made with `connect_keep_trying()` against a
//! generated server on the simulated WebSocket (TR-22, `tests_common::start_device_async_ws`), on tokio's paused
//! clock. The server starts late, goes away and comes back while an observer keeps delivering; addresses are tried in
//! order; the backoff and the give-up time are the configured ones; `retry_now()` hurries a waiting connection,
//! restarts an attempt under way and pings a running link.

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::sync::broadcast;
    use wire_weaver::prelude::*;
    use wire_weaver_client::observe::{ObserveEvent, periodic as every};
    use wire_weaver_client::{
        Backoff, ConnectionEvent, ConnectionState, Error, GapReason, KeepTrying, RetryNow,
    };
    use ww_si::Fixed;
    use ww_si::prefix::One;
    use ww_si::units::Item;

    type Ticks = Fixed<Item, One, u32>;

    mod server {
        use super::*;

        pub struct Server {
            /// Polls so far, one per millisecond while a host is connected
            pub ticks: u32,
            pub observe: api_impl::ObserveState,
            pub buf: Vec<u8>,
        }

        impl Server {
            pub fn new() -> Self {
                Server {
                    ticks: 0,
                    observe: api_impl::ObserveState::new(Default::default()),
                    buf: vec![0; 256],
                }
            }

            fn observe_state(&mut self) -> &mut api_impl::ObserveState {
                &mut self.observe
            }

            async fn get_ticks(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
            ) -> GetResult<Ticks, CommonError<'_>> {
                GetResult::Value(Fixed::new(self.ticks))
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                keep_trying_api :: Counter for super::Server,
                server = true, no_alloc = true, use_async = true,
                method_model = "_=immediate",
                property_model = "_=get_set",
            );
        }

        impl tests_common::TestProcessEventsAsync for Server {
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
                self.ticks += 1;
                let mut buf = core::mem::take(&mut self.buf);
                _ = self.poll_properties(out, (), now_ms * 1000, &mut buf).await;
                self.buf = buf;
            }
        }
    }

    mod client {
        use wire_weaver_client::{ClientConfig, Commander, WwClient};

        pub struct Client {
            pub cmd: Commander,
        }

        impl WwClient for Client {
            fn default_config() -> ClientConfig {
                ClientConfig::default()
            }

            fn try_from_cmd(cmd: Commander) -> Result<Self, wire_weaver_client::Error> {
                Ok(Self { cmd })
            }
        }

        pub mod api_client {
            wire_weaver::ww_codegen!(
                keep_trying_api :: Counter for super::Client,
                client = "std_client",
            );
        }
    }

    use client::Client;
    use client::api_client::keep_trying_api_counter::CounterUpdate;

    const MS: Duration = Duration::from_millis(1);

    fn start_device(path: &str) -> tests_common::TestDevice {
        tests_common::start_device_async_ws(
            path,
            server::Server::new(),
            keep_trying_api::COUNTER_FULL_GID,
            keep_trying_api::COUNTER_WIRE_VERSION,
            tests_common::api!(server::api_impl),
        )
    }

    /// A keep-trying client of `paths`, in order, steering no clock unless `clock_sync`.
    fn keep_trying(paths: &[&str], keep_trying: KeepTrying, clock_sync: bool) -> Client {
        Client::config(|c| {
            c.in_process_paths(paths.iter().copied())
                .resolver(tests_common::resolver())
                .keep_trying(keep_trying)
                .clock_sync(clock_sync)
        })
        .connect_keep_trying()
        .expect("the handle is made at once")
    }

    /// The next event that isn't a [ConnectionEvent::Connecting] or [ConnectionEvent::AttemptFailed].
    async fn next_outcome(events: &mut broadcast::Receiver<ConnectionEvent>) -> ConnectionEvent {
        loop {
            match events.recv().await.expect("the connection's events") {
                ConnectionEvent::Connecting { .. } | ConnectionEvent::AttemptFailed { .. } => {}
                event => return event,
            }
        }
    }

    /// Waits until `n` more attempts failed.
    async fn failed_attempts(events: &mut broadcast::Receiver<ConnectionEvent>, n: usize) {
        let mut failed = 0;
        while failed < n {
            match events.recv().await.expect("the connection's events") {
                ConnectionEvent::AttemptFailed { .. } => failed += 1,
                ConnectionEvent::Connecting { .. } => {}
                event => panic!("expected failed attempts, got {event:?}"),
            }
        }
    }

    /// The wait after the next failed attempt.
    async fn next_failure(events: &mut broadcast::Receiver<ConnectionEvent>) -> Duration {
        loop {
            match events.recv().await.expect("the connection's events") {
                ConnectionEvent::AttemptFailed { retry_in, .. } => return retry_in,
                ConnectionEvent::Connecting { .. } => {}
                event => panic!("expected a failed attempt, got {event:?}"),
            }
        }
    }

    /// The next update of `ticks`, past a `Disconnected` and a `Resumed`.
    async fn next_tick(
        observer: &mut wire_weaver_client::observe::Observer<CounterUpdate, Ticks>,
    ) -> u32 {
        loop {
            match observer.recv_event().await.expect("observer goes on") {
                ObserveEvent::Update(u) => return u.value.value,
                ObserveEvent::Disconnected | ObserveEvent::Resumed => {}
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    async fn run(test: impl Future<Output = ()>) {
        tokio::task::LocalSet::new().run_until(test).await
    }

    /// The server starts after the client, goes away and comes back: the handle is there from the start, calls fail
    /// with `Disconnected` while there is no link, and an observer opened on the first connect keeps delivering after
    /// the reconnect.
    #[tokio::test(start_paused = true)]
    async fn server_starts_late_drops_and_comes_back() {
        run(async {
            const PATH: &str = "keep_trying/late_drop_back";
            let client = keep_trying(&[PATH], KeepTrying::default(), false);
            let mut events = client.cmd.connection_events();
            let state = client
                .cmd
                .connection_state()
                .expect("a keep-trying connection");
            assert!(matches!(
                *state.borrow(),
                ConnectionState::Connecting { .. }
            ));
            assert!(matches!(
                client.read_ticks().read().await,
                Err(Error::Disconnected)
            ));

            failed_attempts(&mut events, 3).await;
            let device = start_device(PATH);
            match next_outcome(&mut events).await {
                ConnectionEvent::Connected { address, attempts } => {
                    assert_eq!(address, format!("in_process {PATH}"));
                    assert_eq!(attempts, 4);
                }
                event => panic!("expected Connected, got {event:?}"),
            }
            assert_eq!(
                *state.borrow(),
                ConnectionState::Connected {
                    address: format!("in_process {PATH}")
                }
            );
            assert_eq!(
                client.cmd.device_api_info().user_api_version.crate_id,
                "keep_trying_api"
            );

            let mut ticks = client
                .observe_ticks(every(10 * MS))
                .subscribe()
                .await
                .expect("observe");
            let first = next_tick(&mut ticks).await;
            assert!(next_tick(&mut ticks).await > first, "counts up");

            drop(device);
            match next_outcome(&mut events).await {
                ConnectionEvent::Disconnected {
                    reason: GapReason::Lost(_),
                    reconnecting: true,
                } => {}
                event => panic!("expected Disconnected, got {event:?}"),
            }
            assert!(matches!(
                client.read_ticks().read().await,
                Err(Error::Disconnected)
            ));
            failed_attempts(&mut events, 2).await;

            // a new server under the same path: its count starts over
            let _device = start_device(PATH);
            match next_outcome(&mut events).await {
                ConnectionEvent::Reconnected {
                    attempts, resumed, ..
                } => {
                    assert_eq!(attempts, 3);
                    assert_eq!(resumed, 1, "the observed level");
                }
                event => panic!("expected Reconnected, got {event:?}"),
            }
            let after = next_tick(&mut ticks).await;
            assert!(next_tick(&mut ticks).await > after, "counts up again");
            let read = client.read_ticks().read().await.expect("calls go through");
            assert!(read.value > 0);

            client.cmd.disconnect().asynch().await.expect("disconnect");
            assert_eq!(*state.borrow(), ConnectionState::Disconnected);
        })
        .await
    }

    /// Each attempt tries the addresses in order, the first that has a server wins; the clock steering prepared
    /// before the first connect starts on it.
    #[tokio::test(start_paused = true)]
    async fn first_address_that_answers_wins() {
        run(async {
            const ABSENT: &str = "keep_trying/order_absent";
            const PRESENT: &str = "keep_trying/order_present";
            let _device = start_device(PRESENT);
            let client = keep_trying(&[ABSENT, PRESENT], KeepTrying::default(), true);
            let mut events = client.cmd.connection_events();
            match next_outcome(&mut events).await {
                ConnectionEvent::Connected { address, attempts } => {
                    assert_eq!(address, format!("in_process {PRESENT}"));
                    assert_eq!(attempts, 1);
                }
                event => panic!("expected Connected, got {event:?}"),
            }
            assert!(client.cmd.clock_sync().is_some(), "steering prepared");
        })
        .await
    }

    /// The wait between attempts follows the configured backoff, and the client gives up after the configured time.
    #[tokio::test(start_paused = true)]
    async fn backoff_and_give_up_are_the_settings() {
        run(async {
            let settings = KeepTrying {
                backoff: Backoff {
                    first: 50 * MS,
                    max: 300 * MS,
                },
                give_up: Some(Duration::from_secs(2)),
            };
            let client = keep_trying(&["keep_trying/never"], settings, false);
            let mut events = client.cmd.connection_events();
            let mut waits = vec![];
            loop {
                match events.recv().await.expect("the connection's events") {
                    ConnectionEvent::AttemptFailed { retry_in, .. } => waits.push(retry_in),
                    ConnectionEvent::Connecting { .. } => {}
                    ConnectionEvent::GaveUp { .. } => break,
                    event => panic!("unexpected {event:?}"),
                }
            }
            assert_eq!(
                waits[..5],
                [50 * MS, 100 * MS, 200 * MS, 300 * MS, 300 * MS]
            );
            assert!(waits[5..].iter().all(|w| *w == 300 * MS), "{waits:?}");
            assert_eq!(
                *client.cmd.connection_state().unwrap().borrow(),
                ConnectionState::Disconnected
            );
        })
        .await
    }

    /// Waiting between attempts, `retry_now()` starts the next one at once and the backoff over from its first wait.
    #[tokio::test(start_paused = true)]
    async fn retry_now_hurries_a_waiting_connection() {
        run(async {
            const PATH: &str = "keep_trying/retry_waiting";
            let settings = KeepTrying {
                backoff: Backoff {
                    first: Duration::from_secs(1),
                    max: Duration::from_secs(60),
                },
                give_up: None,
            };
            let client = keep_trying(&[PATH], settings, false);
            let mut events = client.cmd.connection_events();
            let mut waits = vec![];
            for _ in 0..3 {
                waits.push(next_failure(&mut events).await);
            }
            assert_eq!(waits.last(), Some(&Duration::from_secs(4)));

            let asked = tokio::time::Instant::now();
            assert_eq!(client.cmd.retry_now().await, RetryNow::Attempting);
            assert_eq!(
                next_failure(&mut events).await,
                Duration::from_secs(1),
                "the backoff starts over"
            );
            assert_eq!(asked.elapsed(), Duration::ZERO, "the attempt ran at once");

            let _device = start_device(PATH);
            let asked = tokio::time::Instant::now();
            assert_eq!(client.cmd.retry_now().await, RetryNow::Attempting);
            match next_outcome(&mut events).await {
                ConnectionEvent::Connected { attempts, .. } => assert_eq!(attempts, 5),
                event => panic!("expected Connected, got {event:?}"),
            }
            assert!(
                asked.elapsed() < Duration::from_millis(100),
                "connected long before the 1 s wait: {:?}",
                asked.elapsed()
            );
        })
        .await
    }

    /// During an attempt, `retry_now()` stops it and starts a new one, on the same command channel: the client
    /// still connects after.
    #[tokio::test(start_paused = true)]
    async fn retry_now_restarts_an_attempt_under_way() {
        run(async {
            const PATH: &str = "keep_trying/retry_under_way";
            // registered and never served: the WebSocket handshake of an attempt never ends
            let port = wire_weaver_client::in_process::ws_device(PATH);
            let client = keep_trying(&[PATH], KeepTrying::default(), false);
            let mut events = client.cmd.connection_events();
            assert_eq!(
                events.recv().await.expect("an event"),
                ConnectionEvent::Connecting { attempt: 1 }
            );
            assert_eq!(client.cmd.retry_now().await, RetryNow::Restarted);
            assert_eq!(
                events.recv().await.expect("an event"),
                ConnectionEvent::Connecting { attempt: 2 }
            );

            // the attempt under way fails when its device goes
            drop(port);
            next_failure(&mut events).await;
            let _device = start_device(PATH);
            assert_eq!(client.cmd.retry_now().await, RetryNow::Attempting);
            match next_outcome(&mut events).await {
                ConnectionEvent::Connected { attempts, .. } => assert_eq!(attempts, 3),
                event => panic!("expected Connected, got {event:?}"),
            }
            assert!(client.read_ticks().read().await.is_ok(), "calls go through");
        })
        .await
    }

    /// Connected, `retry_now()` pings the device and the link goes on; a connection that doesn't reconnect by itself
    /// (stopped by `disconnect()`, a plain `connect()`) is left as it is.
    #[tokio::test(start_paused = true)]
    async fn retry_now_pings_a_running_link_and_leaves_others() {
        run(async {
            const PATH: &str = "keep_trying/retry_running";
            let _device = start_device(PATH);
            let client = keep_trying(&[PATH], KeepTrying::default(), false);
            let mut events = client.cmd.connection_events();
            assert!(matches!(
                next_outcome(&mut events).await,
                ConnectionEvent::Connected { .. }
            ));
            assert_eq!(client.cmd.retry_now().await, RetryNow::Pinged);
            assert!(client.read_ticks().read().await.is_ok(), "the link goes on");

            client.cmd.disconnect().asynch().await.expect("disconnect");
            assert_eq!(client.cmd.retry_now().await, RetryNow::NotTrying);

            let plain = Client::config(|c| {
                c.in_process_path(PATH.into())
                    .resolver(tests_common::resolver())
            })
            .connect()
            .await
            .expect("connects");
            assert_eq!(plain.cmd.retry_now().await, RetryNow::NotTrying);
        })
        .await
    }
}

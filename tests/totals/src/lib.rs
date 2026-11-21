//! Running totals over plain sinks (API-70) end to end: `TotalsSender` on the host, `TotalsRx` on the device.
//!
//! - Over simulated UDP that drops and reorders datagrams, every change made after a session's first write arrives
//!   (`lost_and_reordered_datagrams_lose_no_change`), and the device sees the totals only grow: `ww_link`'s UDP
//!   framing drops late and duplicated datagrams, so the writes need no sequence number of their own.
//! - Datagrams held back further than the link's frame counter tells apart can arrive out of order: a step back,
//!   then the next write brings the rest, and nothing is lost (`reordering_past_the_link_window_settles`).
//! - Over the simulated WebSocket, and with a second sender to a device that kept its receiver: the new session's
//!   first write only sets where its totals start (`a_new_sender_starts_a_session`).
//!
//! Everything runs on tokio's paused clock: the outcome depends on the datagram plan alone.
#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tests_common::{Fate, TestDevice};
    use totals_api::{Delta, Detents, Px};
    use wire_weaver::prelude::*;
    use wire_weaver_client::{ConnectionState, KeepTrying, TotalsSender};
    use ww_stream::totals::{TotalsRx, TotalsRxStats};

    /// What the device applied, and how.
    #[derive(Default)]
    struct Seen {
        rx: TotalsRx<Delta>,
        /// The sum of every change applied.
        applied: Delta,
        /// Changes that moved back on an axis.
        steps_back: usize,
    }

    mod server {
        use super::*;
        use tests_common::TestProcessEventsAsync;

        pub struct Server {
            pub seen: Arc<Mutex<Seen>>,
        }

        impl Server {
            async fn write_motion(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                motion: Totals<Delta>,
            ) {
                let mut seen = self.seen.lock().unwrap();
                if let Some(change) = seen.rx.receive(motion) {
                    seen.applied = seen.applied.accumulate(change);
                    if change.dx.get() < 0 || change.dy.get() < 0 {
                        seen.steps_back += 1;
                    }
                }
            }

            async fn sideband_motion(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }

            async fn write_scroll(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                _scroll: Totals<Detents>,
            ) {
            }

            async fn sideband_scroll(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                totals_api :: Pointer for super::Server,
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
                totals_api :: Pointer for super::Client,
                client = "std_client",
            );
        }
    }

    use client::Client;

    const MS: Duration = Duration::from_millis(1);

    fn delta(dx: i32, dy: i32) -> Delta {
        Delta {
            dx: Px::new(dx),
            dy: Px::new(dy),
        }
    }

    /// Datagrams from the host while `lossy` is set: every `drop_every`th dropped, every `delay_every`th held back
    /// behind `delay` later ones.
    struct Plan {
        lossy: Arc<AtomicBool>,
        datagrams: Arc<AtomicUsize>,
        drop_every: usize,
        delay_every: usize,
        delay: usize,
    }

    impl Plan {
        fn impair(&self) -> tests_common::Impair {
            let (lossy, datagrams) = (self.lossy.clone(), self.datagrams.clone());
            let (drop_every, delay_every, delay) = (self.drop_every, self.delay_every, self.delay);
            Box::new(move |_| {
                if !lossy.load(Ordering::Relaxed) {
                    return Fate::Deliver;
                }
                let n = datagrams.fetch_add(1, Ordering::Relaxed) + 1;
                if n % drop_every == 0 {
                    Fate::Drop
                } else if n % delay_every == 0 {
                    Fate::Delay(delay)
                } else {
                    Fate::Deliver
                }
            })
        }
    }

    fn start_udp(path: &str, plan: &Plan, seen: &Arc<Mutex<Seen>>) -> TestDevice {
        tests_common::start_device_async_udp(
            path,
            server::Server { seen: seen.clone() },
            totals_api::POINTER_FULL_GID,
            totals_api::POINTER_WIRE_VERSION,
            tests_common::api!(server::api_impl),
            plan.impair(),
        )
    }

    fn start_ws(path: &str, seen: &Arc<Mutex<Seen>>) -> TestDevice {
        tests_common::start_device_async_ws(
            path,
            server::Server { seen: seen.clone() },
            totals_api::POINTER_FULL_GID,
            totals_api::POINTER_WIRE_VERSION,
            tests_common::api!(server::api_impl),
        )
    }

    fn connect(device: &TestDevice) -> Client {
        Client::config(|c| {
            c.in_process_path(device.path())
                .resolver(tests_common::resolver())
                .keep_trying(KeepTrying::default())
        })
        .connect_keep_trying()
        .expect("the handle is made at once")
    }

    /// A sender running on `client`'s motion sink, once connected.
    async fn start_sender(client: &Client) -> TotalsSender<Delta> {
        let mut state = client.cmd.connection_state().expect("keeps trying");
        state
            .wait_for(|s| matches!(s, ConnectionState::Connected { .. }))
            .await
            .expect("connects");
        let sender = TotalsSender::new();
        let sink = client.motion().await.expect("the motion sink");
        let running = sender.clone();
        tokio::task::spawn_local(async move { running.run(sink, state).await });
        sender
    }

    /// Waits on the paused clock until the device took the first write of its `n`th session.
    async fn session_started(seen: &Arc<Mutex<Seen>>, n: u32) {
        for _ in 0..10_000 {
            if seen.lock().unwrap().rx.stats().sessions == n {
                return;
            }
            tokio::time::sleep(MS).await;
        }
        panic!(
            "session {n} not started: {:?}",
            seen.lock().unwrap().rx.stats()
        );
    }

    /// Waits on the paused clock until the device applied `want` since the first write of the session.
    async fn settles(seen: &Arc<Mutex<Seen>>, want: Delta) {
        for _ in 0..10_000 {
            if seen.lock().unwrap().applied == want {
                return;
            }
            tokio::time::sleep(MS).await;
        }
        panic!("applied {:?}, want {want:?}", seen.lock().unwrap().applied);
    }

    /// Moves in bursts, a millisecond apart: some go out alone, others merge while a write is on its way.
    async fn moves(sender: &TotalsSender<Delta>, n: i32) -> Delta {
        let mut sum = Delta::default();
        for i in 0..n {
            let change = delta(i % 5 + 1, i % 3);
            sender.add(change);
            sum = sum.accumulate(change);
            if i % 3 == 0 {
                tokio::time::sleep(MS).await;
            }
        }
        sum
    }

    async fn run(test: impl Future<Output = ()>) {
        tokio::task::LocalSet::new().run_until(test).await
    }

    #[tokio::test(start_paused = true)]
    async fn lost_and_reordered_datagrams_lose_no_change() {
        run(async {
            let seen = Arc::new(Mutex::new(Seen::default()));
            let plan = Plan {
                lossy: Arc::default(),
                datagrams: Arc::default(),
                drop_every: 4,
                delay_every: 7,
                delay: 3,
            };
            let device = start_udp("totals/udp_lossy", &plan, &seen);
            let client = connect(&device);
            let sender = start_sender(&client).await;
            // the session's first write is where the device's totals start
            session_started(&seen, 1).await;

            plan.lossy.store(true, Ordering::Relaxed);
            let sum = moves(&sender, 600).await;
            settles(&seen, sum).await;

            let (tx, seen) = (sender.stats(), seen.lock().unwrap());
            assert!(plan.datagrams.load(Ordering::Relaxed) > 100, "{tx:?}");
            assert_eq!(seen.steps_back, 0, "the link keeps the writes in order");
            assert!(tx.merged > 0, "{tx:?}");
            assert!(
                seen.rx.stats().writes < tx.writes + tx.repeats,
                "writes lost: {tx:?} {:?}",
                seen.rx.stats()
            );
            assert_eq!(tx.unsent, 0);
        })
        .await
    }

    #[tokio::test(start_paused = true)]
    async fn reordering_past_the_link_window_settles() {
        run(async {
            let seen = Arc::new(Mutex::new(Seen::default()));
            let plan = Plan {
                lossy: Arc::default(),
                datagrams: Arc::default(),
                drop_every: 23,
                delay_every: 11,
                delay: 12,
            };
            let device = start_udp("totals/udp_deep_reorder", &plan, &seen);
            let client = connect(&device);
            let sender = start_sender(&client).await;
            session_started(&seen, 1).await;

            plan.lossy.store(true, Ordering::Relaxed);
            let mut sum = moves(&sender, 600).await;
            // Until TR-33 the link's 4-bit frame counter takes a datagram 12 behind for one ahead, and then drops
            // up to 8 frames in order as late: the last writes and their repeats can be among them. The next
            // changes bring everything once the counter came round.
            plan.lossy.store(false, Ordering::Relaxed);
            for _ in 0..20 {
                sender.add(delta(1, 0));
                sum = sum.accumulate(delta(1, 0));
                tokio::time::sleep(10 * MS).await;
            }
            settles(&seen, sum).await;
            assert!(
                seen.lock().unwrap().steps_back > 0,
                "reordered past the link"
            );
        })
        .await
    }

    #[tokio::test(start_paused = true)]
    async fn a_new_sender_starts_a_session() {
        run(async {
            let seen = Arc::new(Mutex::new(Seen::default()));
            let device = start_ws("totals/ws_second_sender", &seen);

            let first = connect(&device);
            let one = start_sender(&first).await;
            session_started(&seen, 1).await;
            let sum = moves(&one, 100).await;
            settles(&seen, sum).await;
            first.cmd.disconnect().asynch().await.expect("disconnect");

            // the device kept its receiver: a new program's totals start over, its first write is its start
            let second = connect(&device);
            let sender = start_sender(&second).await;
            session_started(&seen, 2).await;
            sender.add(delta(1000, 1000));
            let more = moves(&sender, 100).await;
            settles(&seen, sum.accumulate(delta(1000, 1000)).accumulate(more)).await;
            let seen = seen.lock().unwrap();
            assert_eq!(seen.rx.stats().sessions, 2);
            assert_eq!(seen.steps_back, 0);
        })
        .await
    }

    /// Writes counted by both ends over a lossless link: one per change that went alone, and the merged ones.
    #[tokio::test(start_paused = true)]
    async fn stats_add_up_on_a_lossless_link() {
        run(async {
            let seen = Arc::new(Mutex::new(Seen::default()));
            let device = start_ws("totals/ws_stats", &seen);
            let client = connect(&device);
            let sender = start_sender(&client).await;
            session_started(&seen, 1).await;
            let sum = moves(&sender, 300).await;
            settles(&seen, sum).await;
            tokio::time::sleep(500 * MS).await;
            let tx = sender.stats();
            let rx: TotalsRxStats = seen.lock().unwrap().rx.stats();
            assert_eq!(rx.writes, tx.writes + tx.repeats, "{tx:?} {rx:?}");
            assert_eq!(tx.adds, 300);
            assert_eq!(rx.sessions, 1);
            assert_eq!(
                rx.unchanged, tx.repeats,
                "a repeat over a lossless link changes nothing"
            );
            assert_eq!(tx.adds, tx.merged + tx.writes - 1 + tx.unsent, "{tx:?}");
        })
        .await
    }
}

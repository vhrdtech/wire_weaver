//! Observed properties end to end (SRV-18): `Observe` requests and `PropertyUpdate` events between the generated
//! server, polled with a fake clock, and the real host event loop (`tests_common`). Requests go out raw through the
//! `Commander` and updates are read from a tracer, until the typed host side (CLT-28) exists.

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;
    use tests_common::TestDevice;
    use tokio::sync::{mpsc, oneshot};
    use wire_weaver::prelude::*;
    use wire_weaver::shrink_wrap::UVlq32Backfill;
    use wire_weaver_client::internal::Command;
    use ww_client_server::observe::{
        ObserveEntry, ObserveGranted, ObserveMode, ObserveRefusal, ObserveRefused, ObserveRequest,
    };
    use ww_client_server::{Event, EventKind, PathKind, Request, RequestKind};
    use ww_stream::observe::ObserveLimits;

    /// Resource indices of the API
    const TEMP: u32 = 0;
    const MODE: u32 = 1;
    const PLAIN: u32 = 2;
    const LABEL: u32 = 3;
    const MOTOR: u32 = 5;
    const BANK: u32 = 6;
    const SPEED: u32 = 0;
    const CURRENT: u32 = 1;
    const POSITION: u32 = 2;

    mod server {
        use super::*;
        use tests_common::TestProcessEventsAsync;

        pub struct Server {
            pub mode: u8,
            pub temp: f32,
            pub label: String,
            pub observe: api_impl::ObserveState,
            /// Fake microsecond clock, set by the test
            pub clock: Arc<AtomicU64>,
            /// Last fake time `poll_properties` ran at
            pub polled: Arc<AtomicU64>,
            pub buf: Vec<u8>,
        }

        impl Server {
            fn observe_state(&mut self) -> &mut api_impl::ObserveState {
                &mut self.observe
            }

            async fn get_temp(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
            ) -> GetResult<f32, ()> {
                GetResult::Value(self.temp)
            }

            async fn changed_mode(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

            async fn set_plain(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                _value: u8,
            ) -> SetResult<()> {
                SetResult::Set
            }

            async fn get_plain(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
            ) -> GetResult<u8, ()> {
                GetResult::Value(0)
            }

            async fn get_label(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
            ) -> GetResult<&'_ str, ()> {
                GetResult::Value(&self.label)
            }

            async fn set_temp(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                value: f32,
            ) -> RpcResult<()> {
                self.temp = value;
                self.mark_changed_temp();
                RpcResult::Ready(())
            }

            fn valid_indices_root_bank(&mut self) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..2)
            }

            /// Motor values follow the clock: speed in ms, current in 10 ms, position in 100 ms
            fn now(&self) -> u64 {
                self.clock.load(Ordering::Relaxed)
            }

            async fn get_motor_speed(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
            ) -> GetResult<u32, ()> {
                GetResult::Value((self.now() / 1_000) as u32)
            }

            async fn get_motor_current(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
            ) -> GetResult<u32, ()> {
                GetResult::Value((self.now() / 10_000) as u32)
            }

            async fn get_motor_position(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
            ) -> GetResult<u32, ()> {
                GetResult::Value((self.now() / 100_000) as u32)
            }

            async fn get_bank_level(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                _index_chain: [UNib32; 1],
            ) -> GetResult<u8, ()> {
                GetResult::Value(0)
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                observe_api :: Observe for super::Server,
                server = true, no_alloc = true, use_async = true,
                method_model = "_=immediate",
                property_model = "mode=value_on_changed, _=get_set",
                // debug_to_file = "../../target/tests_observe_server.rs"
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

            async fn poll(&mut self, out: &mut impl EventOut, _now_ms: u64) {
                let now = self.now();
                let mut buf = core::mem::take(&mut self.buf);
                _ = self.poll_properties(out, (), now, &mut buf).await;
                self.buf = buf;
                self.polled.store(now, Ordering::Relaxed);
            }
        }
    }

    /// The same API on a blocking server (`use_async = false`): `poll_properties` without async
    mod blocking_server {
        use super::*;

        pub struct Server {
            pub mode: u8,
            pub observe: api_impl::ObserveState,
        }

        impl Server {
            fn observe_state(&mut self) -> &mut api_impl::ObserveState {
                &mut self.observe
            }

            fn get_temp(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<f32, ()> {
                GetResult::Value(0.5)
            }

            fn changed_mode(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn set_plain(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _value: u8,
            ) -> SetResult<()> {
                SetResult::Set
            }

            fn get_plain(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<u8, ()> {
                GetResult::Value(0)
            }

            fn get_label(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<&'_ str, ()> {
                GetResult::Unimplemented
            }

            fn set_temp(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _value: f32,
            ) -> RpcResult<()> {
                self.mark_changed_temp();
                RpcResult::Ready(())
            }

            fn valid_indices_root_bank(&mut self) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..0)
            }

            fn get_motor_speed(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<u32, ()> {
                GetResult::Value(1)
            }

            fn get_motor_current(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<u32, ()> {
                GetResult::Value(2)
            }

            fn get_motor_position(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<u32, ()> {
                GetResult::Value(3)
            }

            fn get_bank_level(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _index_chain: [UNib32; 1],
            ) -> GetResult<u8, ()> {
                GetResult::Value(0)
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                observe_api :: Observe for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "mode=value_on_changed, _=get_set",
            );
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

            fn from_cmd(cmd: Commander) -> Self {
                Self { cmd }
            }
        }

        mod api_client {
            wire_weaver::ww_codegen!(
                observe_api :: Observe for super::Client,
                client = "std_client",
            );
        }
    }

    use client::Client;
    use server::api_impl::{MotorUpdate, ObserveUpdate};

    /// A `PropertyUpdate` as the host got it
    #[derive(Debug, Clone)]
    struct Update {
        path: Vec<u32>,
        t: u64,
        missed: u32,
        data: Vec<u8>,
        /// Length of the whole event
        len: usize,
    }

    struct Bench {
        _device: TestDevice,
        client: Client,
        clock: Arc<AtomicU64>,
        polled: Arc<AtomicU64>,
        updates: mpsc::UnboundedReceiver<Update>,
    }

    impl Bench {
        async fn start(path: &str, limits: ObserveLimits) -> Bench {
            let _ = tracing_subscriber::fmt::try_init();
            let clock = Arc::new(AtomicU64::new(0));
            let polled = Arc::new(AtomicU64::new(0));
            let server = server::Server {
                mode: 0,
                temp: 0.0,
                label: "hello".into(),
                observe: server::api_impl::ObserveState::new(limits),
                clock: clock.clone(),
                polled: polled.clone(),
                buf: vec![0; 512],
            };
            let device = tests_common::start_device_async(
                path,
                server,
                observe_api::OBSERVE_FULL_GID,
                server::api_impl::api_hash(),
            );
            let client = Client::config(|c| c.in_process_path(device.path()))
                .connect()
                .await
                .expect("connect");
            let (trace_tx, mut trace_rx) = mpsc::unbounded_channel();
            client
                .cmd
                .send(Command::RegisterTracer {
                    trace_event_tx: trace_tx,
                })
                .await
                .unwrap();
            let (updates_tx, updates) = mpsc::unbounded_channel();
            tokio::spawn(async move {
                while let Some(ev) = trace_rx.recv().await {
                    let wire_weaver_client::internal::TraceEvent::Event { bytes } = ev else {
                        continue;
                    };
                    let Ok(event) = Event::from_ww_bytes(&bytes) else {
                        continue;
                    };
                    if let Ok(EventKind::PropertyUpdate {
                        path,
                        t,
                        missed,
                        data,
                    }) = event.result
                    {
                        let t = match t.map(|t| t.value) {
                            Some(ww_si::ww_numeric::NumericValue::U64(t)) => t,
                            t => panic!("timestamp {t:?}"),
                        };
                        _ = updates_tx.send(Update {
                            path: path.iter().map(|p| p.unwrap().0).collect(),
                            t,
                            missed: missed.map_or(0, |m| m.0),
                            data: data.0.to_vec(),
                            len: bytes.len(),
                        });
                    }
                }
            });
            Bench {
                _device: device,
                client,
                clock,
                polled,
                updates,
            }
        }

        /// Send an `Observe` request to the level at `path`: the granted settings or the error.
        async fn observe(
            &self,
            path: &[u32],
            entries: &[ObserveEntry],
            patch: bool,
            budget: Option<u32>,
        ) -> Result<(Vec<ObserveEntry>, Vec<ObserveRefused>, u32), String> {
            let path: Vec<UNib32> = path.iter().map(|p| UNib32(*p)).collect();
            let req = Request {
                seq: UVlq32Backfill(0),
                path_kind: PathKind::Absolute {
                    path: RefVec::Slice { slice: &path },
                },
                kind: RequestKind::Observe {
                    cfg: ObserveRequest {
                        entries: RefVec::Slice { slice: entries },
                        patch,
                        budget: budget.map(UNib32),
                    },
                },
            };
            let mut buf = [0u8; 512];
            let bytes = req.to_ww_bytes(&mut buf).unwrap().to_vec();
            self.raw(bytes).await.map(|data| {
                let granted = ObserveGranted::from_ww_bytes(&data).unwrap();
                (
                    granted.entries.iter().map(|e| e.unwrap()).collect(),
                    granted.refused.iter().map(|e| e.unwrap()).collect(),
                    granted.budget.0,
                )
            })
        }

        async fn raw(&self, bytes: Vec<u8>) -> Result<Vec<u8>, String> {
            let (done_tx, done_rx) = oneshot::channel();
            self.client
                .cmd
                .send(Command::SendMessage {
                    bytes,
                    done_tx: Some((done_tx, Duration::from_secs(2))),
                })
                .await
                .unwrap();
            done_rx.await.unwrap().map_err(|e| format!("{e:?}"))
        }

        /// Move the fake clock to `t` and wait until the device polled at it.
        async fn at(&self, t: u64) {
            self.clock.store(t, Ordering::Relaxed);
            let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
            while self.polled.load(Ordering::Relaxed) < t {
                assert!(tokio::time::Instant::now() < deadline, "device polls");
                tokio::time::sleep(Duration::from_micros(200)).await;
            }
        }

        /// Run the clock from `from` to `to` in `step`s.
        async fn run(&self, from: u64, to: u64, step: u64) {
            let mut t = from;
            while t <= to {
                self.at(t).await;
                t += step;
            }
        }

        /// Everything received until nothing came for 100 ms.
        async fn drain(&mut self) -> Vec<Update> {
            let mut out = vec![];
            while let Ok(Some(u)) =
                tokio::time::timeout(Duration::from_millis(100), self.updates.recv()).await
            {
                out.push(u);
            }
            out.sort_by_key(|u| u.t);
            out
        }
    }

    fn periodic(index: u32, every_us: u32) -> ObserveEntry {
        ObserveEntry {
            index: UNib32(index),
            mode: ObserveMode::Periodic {
                every_us: UNib32(every_us),
            },
        }
    }

    fn on_change(index: u32, min_us: u32) -> ObserveEntry {
        ObserveEntry {
            index: UNib32(index),
            mode: ObserveMode::OnChange {
                min_us: UNib32(min_us),
            },
        }
    }

    fn limits() -> ObserveLimits {
        ObserveLimits {
            timestamps: true,
            ..ObserveLimits::DEFAULT
        }
    }

    /// Send times of each motor property
    fn motor_sends(updates: &[Update]) -> BTreeMap<&'static str, Vec<u64>> {
        let mut sends: BTreeMap<&'static str, Vec<u64>> = BTreeMap::new();
        for u in updates.iter().filter(|u| u.path == [MOTOR]) {
            let m = MotorUpdate::from_ww_bytes(&u.data).unwrap();
            if let Some(speed) = m.speed {
                assert_eq!(speed as u64, u.t / 1_000, "a fresh value");
                sends.entry("speed").or_default().push(u.t);
            }
            if m.current.is_some() {
                sends.entry("current").or_default().push(u.t);
            }
            if m.position.is_some() {
                sends.entry("position").or_default().push(u.t);
            }
        }
        sends
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn periodic_rates() {
        let mut b = Bench::start("observe/periodic_rates", limits()).await;
        let (granted, refused, budget) = b
            .observe(
                &[MOTOR],
                &[
                    periodic(SPEED, 10_000),
                    periodic(CURRENT, 20_000),
                    periodic(POSITION, 5_000),
                ],
                false,
                Some(10_000),
            )
            .await
            .unwrap();
        assert_eq!(granted.len(), 3);
        assert!(refused.is_empty());
        assert_eq!(budget, 10_000);
        b.run(1_000, 200_000, 1_000).await;
        let sends = motor_sends(&b.drain().await);
        // the first poll after the request sends all (at 0 or 1 ms, the device polls while the clock still stands
        // at 0), then on the grid from there, held to the period
        let start = sends["speed"][0];
        assert!(start == 0 || start == 1_000, "{start}");
        let grid = |every: u64| {
            (0..)
                .map(move |i| start + i * every)
                .take_while(|t| *t <= 200_000)
                .collect::<Vec<_>>()
        };
        assert_eq!(sends["speed"], grid(10_000));
        assert_eq!(sends["current"], grid(20_000));
        assert_eq!(sends["position"], grid(5_000));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn budget_and_missed() {
        let mut b = Bench::start("observe/budget_and_missed", limits()).await;
        // every ms, 3 values: about 13 kB/s wanted, 1 kB/s granted
        let (_, _, budget) = b
            .observe(
                &[MOTOR],
                &[
                    periodic(SPEED, 1_000),
                    periodic(CURRENT, 1_000),
                    periodic(POSITION, 1_000),
                ],
                false,
                Some(1_000),
            )
            .await
            .unwrap();
        assert_eq!(budget, 1_000);
        b.run(1_000, 500_000, 1_000).await;
        let updates = b.drain().await;
        let bytes: usize = updates.iter().map(|u| u.len).sum();

        // half a second at 1 kB/s, plus the bucket (a tenth of a second of it, full at the start)
        assert!(bytes <= 500 + 100, "{bytes}");
        assert!(bytes >= 400, "{bytes}");
        // any 100 ms: the bucket plus what flowed in
        for u in &updates {
            let window: usize = updates
                .iter()
                .filter(|w| w.t >= u.t && w.t < u.t + 100_000)
                .map(|w| w.len)
                .sum();
            assert!(window <= 100 + 100, "{}: {window}", u.t);
        }
        // every due send up to the last update that wasn't made was reported in an update
        let sends = motor_sends(&updates);
        let sent: usize = sends.values().map(|s| s.len()).sum();
        let missed: u32 = updates.iter().map(|u| u.missed).sum();
        let due = 3 * updates.last().unwrap().t as usize / 1_000;
        assert!(
            missed as usize + sent + 3 >= due,
            "{missed} + {sent} < {due}"
        );
        // earliest deadline first: no property starves
        let counts: Vec<usize> = sends.values().map(|s| s.len()).collect();
        assert_eq!(counts.len(), 3);
        let (min, max) = (counts.iter().min().unwrap(), counts.iter().max().unwrap());
        assert!(max - min <= 2, "{counts:?}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn on_change_and_off() {
        let mut b = Bench::start("observe/on_change_and_off", limits()).await;
        let (granted, _, _) = b
            .observe(
                &[],
                &[on_change(TEMP, 10_000), on_change(MODE, 0)],
                false,
                None,
            )
            .await
            .unwrap();
        // clamped to the 1 ms floor
        assert_eq!(granted, [on_change(TEMP, 10_000), on_change(MODE, 1_000)]);
        // initial values right away
        b.at(1_000).await;
        let first = b.drain().await;
        assert_eq!(first.len(), 1, "{first:?}");
        let u = ObserveUpdate::from_ww_bytes(&first[0].data).unwrap();
        assert_eq!((u.temp, u.mode, u.label), (Some(0.0), Some(0), None));

        // nothing while nothing changes
        b.run(2_000, 20_000, 1_000).await;
        assert!(b.drain().await.is_empty());

        // a method marks temp changed, a write marks mode changed (value_on_changed): sent at the next poll
        b.client.set_temp(1.5).call().await.unwrap();
        b.client.write_mode(7).write().await.unwrap();
        b.at(21_000).await;
        let mut temp_at = None;
        let mut mode = None;
        for u in b.drain().await {
            let v = ObserveUpdate::from_ww_bytes(&u.data).unwrap();
            if let Some(temp) = v.temp {
                assert_eq!(temp, 1.5);
                temp_at = Some(u.t);
            }
            mode = v.mode.or(mode);
        }
        assert_eq!(mode, Some(7));
        let sent_at = temp_at.unwrap();
        assert!((20_000..=21_000).contains(&sent_at), "{sent_at}");

        // temp at most every 10 ms: the change at 22 ms goes 10 ms after the previous send
        b.client.set_temp(2.5).call().await.unwrap();
        b.run(22_000, 40_000, 1_000).await;
        let u = b.drain().await;
        assert_eq!(
            u.iter().map(|u| u.t).collect::<Vec<_>>(),
            [sent_at + 10_000]
        );

        // off: nothing more
        let (granted, _, budget) = b.observe(&[], &[], false, None).await.unwrap();
        assert!(granted.is_empty());
        assert_eq!(budget, 0);
        b.client.set_temp(3.5).call().await.unwrap();
        b.run(41_000, 60_000, 1_000).await;
        assert!(b.drain().await.is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unsized_value() {
        let mut b = Bench::start("observe/unsized_value", limits()).await;
        b.observe(&[], &[periodic(LABEL, 10_000)], false, None)
            .await
            .unwrap();
        b.run(1_000, 21_000, 10_000).await;
        let u = b.drain().await;
        assert_eq!(u.len(), 3);
        for u in u {
            let u = ObserveUpdate::from_ww_bytes(&u.data).unwrap();
            assert_eq!(u.label, Some("hello"));
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn refused_and_errors() {
        let b = Bench::start("observe/refused_and_errors", limits()).await;
        let (granted, refused, _) = b
            .observe(
                &[],
                &[
                    on_change(TEMP, 1_000),
                    on_change(PLAIN, 1_000),
                    on_change(99, 1_000),
                    on_change(TEMP, 5_000),
                    ObserveEntry {
                        index: UNib32(LABEL),
                        mode: ObserveMode::Unknown(9),
                    },
                ],
                false,
                None,
            )
            .await
            .unwrap();
        assert_eq!(granted, [on_change(TEMP, 1_000)]);
        let reasons: Vec<_> = refused.iter().map(|r| (r.index.0, r.reason)).collect();
        assert_eq!(
            reasons,
            [
                (PLAIN, ObserveRefusal::NotObservable),
                (99, ObserveRefusal::NoSuchIndex),
                (TEMP, ObserveRefusal::Duplicate),
                (LABEL, ObserveRefusal::ModeNotSupported),
            ]
        );

        // more refused entries than the answer holds: refused as a whole, nothing changed
        let bad: Vec<_> = (100..140).map(|i| on_change(i, 1_000)).collect();
        let e = b.observe(&[], &bad, false, None).await.unwrap_err();
        assert!(e.contains("BadIndex"), "{e}");
        let (granted, _, _) = b.observe(&[], &[], true, None).await.unwrap();
        assert_eq!(granted, [on_change(TEMP, 1_000)]);

        // a level in a resource array can't be observed yet (API-29)
        let e = b
            .observe(&[BANK, 0], &[on_change(0, 1_000)], false, None)
            .await
            .unwrap_err();
        assert!(e.contains("OperationNotSupported"), "{e}");
        // not a level
        let e = b.observe(&[TEMP], &[], false, None).await.unwrap_err();
        assert!(e.contains("BadPath"), "{e}");
        // a level without observable properties: every entry refused
        // (Motor has them all, so the root's set_temp method stands in)
        let (_, refused, _) = b
            .observe(&[], &[on_change(4, 1_000)], true, None)
            .await
            .unwrap();
        assert_eq!(refused[0].reason, ObserveRefusal::NotObservable);

        // malformed: an Observe request cut short
        let req = Request {
            seq: UVlq32Backfill(0),
            path_kind: PathKind::Absolute {
                path: RefVec::Slice { slice: &[] },
            },
            kind: RequestKind::Observe {
                cfg: ObserveRequest {
                    entries: RefVec::Slice {
                        slice: &[on_change(TEMP, 1_000), on_change(MODE, 1_000)],
                    },
                    patch: false,
                    budget: None,
                },
            },
        };
        let mut buf = [0u8; 64];
        let bytes = req.to_ww_bytes(&mut buf).unwrap();
        // the seq placeholder stays, every cut of the rest is answered, most of them with an error
        let mut errors = 0;
        for cut in 1..bytes.len() - 5 {
            if b.raw(bytes[..bytes.len() - cut].to_vec()).await.is_err() {
                errors += 1;
            }
        }
        assert!(errors > (bytes.len() - 5) / 2, "{errors}");
        // and the device still works
        let (granted, _, _) = b
            .observe(&[], &[on_change(MODE, 1_000)], false, None)
            .await
            .unwrap();
        assert_eq!(granted, [on_change(MODE, 1_000)]);
    }

    /// A blocking server sends the same updates.
    #[test]
    fn blocking_poll() {
        let mut server = blocking_server::Server {
            mode: 3,
            observe: blocking_server::api_impl::ObserveState::new(ObserveLimits::DEFAULT),
        };
        let path = [UNib32(MOTOR)];
        let req = Request {
            seq: UVlq32Backfill(1),
            path_kind: PathKind::Absolute {
                path: RefVec::Slice { slice: &path },
            },
            kind: RequestKind::Observe {
                cfg: ObserveRequest {
                    entries: RefVec::Slice {
                        slice: &[periodic(SPEED, 1_000), periodic(POSITION, 1_000)],
                    },
                    patch: false,
                    budget: None,
                },
            },
        };
        let mut buf = [0u8; 128];
        let req = req.to_ww_bytes(&mut buf).unwrap().to_vec();
        let mut scratch = [0u8; 256];
        let mut queue_buf = [0u8; 256];
        let mut queue = wire_weaver::EventQueue::new(&mut queue_buf);
        let reply = server
            .process_request_bytes(&req, &mut scratch, &mut queue, ())
            .unwrap();
        let reply = Event::from_ww_bytes(reply).unwrap();
        assert!(
            matches!(reply.result, Ok(EventKind::Value { .. })),
            "{reply:?}"
        );

        let mut data = [0u8; 128];
        server
            .poll_properties(&mut queue, (), 0, &mut data)
            .unwrap();
        let events: Vec<_> = queue.iter().map(|e| e.to_vec()).collect();
        assert_eq!(events.len(), 1);
        let Ok(EventKind::PropertyUpdate { data, .. }) =
            Event::from_ww_bytes(&events[0]).unwrap().result
        else {
            panic!();
        };
        let m = MotorUpdate::from_ww_bytes(data.0).unwrap();
        assert_eq!((m.speed, m.current, m.position), (Some(1), None, Some(3)));
    }
}

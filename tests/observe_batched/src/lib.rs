//! Batched observe end to end (API-39): `+observe+batched` properties observed in `Samples` mode, every sample the
//! device takes (or one per `every_us`) sent in batches, `Batched::Series` slots next to `Batched::One` and plain
//! ones. The device runs the generated async server polled with a fake clock, the host the real event loop
//! (`tests_common`); the device side tests send requests raw through the `Commander` and read updates from a tracer.

#[cfg(test)]
mod tests {
    use observe_batched_api::Reading;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;
    use tests_common::TestDevice;
    use tokio::sync::{mpsc, oneshot};
    use wire_weaver::prelude::*;
    use wire_weaver::shrink_wrap::UVlq32Backfill;
    use wire_weaver_client::internal::Command;
    use ww_client_server::observe::{
        Batched, ObserveEntry, ObserveGranted, ObserveMode, ObserveRefusal, ObserveRefused,
        ObserveRequest,
    };
    use ww_client_server::{Event, EventKind, PathKind, Request, RequestKind};
    use ww_si::{Second, Timed};
    use ww_stream::batch::{Scratch, scratch_bytes};
    use ww_stream::observe::ObserveLimits;

    /// Resource indices of the API
    const CURRENT: u32 = 0;
    const READING: u32 = 1;
    const MODE: u32 = 2;
    const SLOW: u32 = 4;
    const TEMP: u32 = 0;

    /// Samples per batch on the device (`observe_batch`)
    const N: usize = 8;
    /// Where batches are rebuilt while they are encoded: the biggest item, `Timed<Reading>`
    const SB: usize = scratch_bytes::<Timed<Reading>>(N);

    const LIMITS: ObserveLimits = ObserveLimits {
        min_period_us: 100,
        default_budget: 100_000,
        max_level_budget: 1_000_000,
        total_budget: 2_000_000,
        timestamps: true,
        samples_max_age_us: (1_000, 1_000_000),
    };

    mod server {
        use super::*;
        use tests_common::TestProcessEventsAsync;

        pub struct Server {
            pub mode: u8,
            /// Last sample taken
            pub current: i32,
            pub observe: api_impl::ObserveState,
            /// Fake microsecond clock, set by the test
            pub clock: Arc<AtomicU64>,
            /// Last fake time `poll_properties` ran at
            pub polled: Arc<AtomicU64>,
            pub buf: Vec<u8>,
            pub scratch: Box<Scratch<SB>>,
        }

        impl Server {
            fn observe_state(&mut self) -> &mut api_impl::ObserveState {
                &mut self.observe
            }

            async fn get_current(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
            ) -> GetResult<i32, ()> {
                GetResult::Value(self.current)
            }

            async fn get_reading(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
            ) -> GetResult<Reading, ()> {
                GetResult::Value(reading(self.current))
            }

            async fn changed_mode(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

            async fn get_slow_temp(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
            ) -> GetResult<f32, ()> {
                GetResult::Value(temp(self.current))
            }

            async fn sample(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                count: u32,
                t0_us: u32,
                dt_us: u32,
                first: i32,
            ) -> RpcResult<()> {
                let mut samplers = self.observe.samplers();
                for i in 0..count {
                    let t = Second::micro(t0_us.wrapping_add(i * dt_us));
                    let v = first + i as i32;
                    samplers.root.sample(
                        t,
                        api_impl::ObserveBatchedSamples {
                            current: v,
                            reading: reading(v),
                        },
                    );
                    if i % 10 == 0 {
                        samplers
                            .root_slow
                            .sample(t, api_impl::SlowSamples { temp: temp(v) });
                    }
                }
                self.current = first + count as i32 - 1;
                RpcResult::Ready(())
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                observe_batched_api :: ObserveBatched for super::Server,
                server = true, no_alloc = true, use_async = true,
                method_model = "_=immediate",
                property_model = "mode=value_on_changed, _=get_set",
                observe_batch = 8,
                observe_outbox = 512,
                // debug_to_file = "../../target/tests_observe_batched_server.rs"
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
                let now = self.clock.load(Ordering::Relaxed);
                let mut buf = core::mem::take(&mut self.buf);
                let mut scratch = core::mem::replace(&mut self.scratch, Box::new(Scratch::new()));
                _ = self
                    .poll_properties(out, (), now, &mut buf, &mut scratch)
                    .await;
                self.buf = buf;
                self.scratch = scratch;
                self.polled.store(now, Ordering::Relaxed);
            }
        }
    }

    fn reading(v: i32) -> Reading {
        Reading {
            value: v * 2,
            ok: v % 2 == 0,
        }
    }

    fn temp(v: i32) -> f32 {
        v as f32 * 0.5
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

        pub mod api_client {
            wire_weaver::ww_codegen!(
                observe_batched_api :: ObserveBatched for super::Client,
                client = "std_client",
                // debug_to_file = "../../target/tests_observe_batched_client.rs"
            );
        }
    }

    use client::Client;
    use server::api_impl::{ObserveBatchedUpdate, SlowUpdate};

    /// A `PropertyUpdate` as the host got it
    #[derive(Debug, Clone)]
    struct Update {
        path: Vec<u32>,
        t: Option<u64>,
        missed: u32,
        data: Vec<u8>,
    }

    impl Update {
        /// Samples of `current` in it: (t µs, value) per item of a series, or the update's t for a single value
        fn current(&self) -> Vec<(Option<u64>, i32)> {
            let u = ObserveBatchedUpdate::from_ww_bytes(&self.data).unwrap();
            match u.current {
                Some(Batched::One(v)) => vec![(self.t, v)],
                Some(Batched::Series(s)) => s.iter().map(|i| (Some(us(i.t)), i.value)).collect(),
                None => vec![],
            }
        }

        fn readings(&self) -> Vec<(u64, Reading)> {
            let u = ObserveBatchedUpdate::from_ww_bytes(&self.data).unwrap();
            match u.reading {
                Some(Batched::Series(s)) => s.iter().map(|i| (us(i.t), i.value)).collect(),
                Some(Batched::One(_)) => panic!("one reading"),
                None => vec![],
            }
        }

        fn mode(&self) -> Option<u8> {
            ObserveBatchedUpdate::from_ww_bytes(&self.data)
                .unwrap()
                .mode
        }

        fn temps(&self) -> Vec<f32> {
            let u = SlowUpdate::from_ww_bytes(&self.data).unwrap();
            match u.temp {
                Some(Batched::Series(s)) => s.iter().map(|i| i.value).collect(),
                Some(Batched::One(v)) => vec![v],
                None => vec![],
            }
        }
    }

    /// Microseconds of a device time as the device sends it (a wrapping u32)
    fn us(t: Second) -> u64 {
        match t.value {
            ww_si::ww_numeric::NumericValue::U32(t) => t as u64,
            t => panic!("timestamp {t:?}"),
        }
    }

    struct Bench {
        _device: TestDevice,
        client: Client,
        clock: Arc<AtomicU64>,
        polled: Arc<AtomicU64>,
        updates: mpsc::UnboundedReceiver<Update>,
    }

    impl Bench {
        async fn start(path: &str, buf_len: usize) -> Bench {
            let _ = tracing_subscriber::fmt::try_init();
            let clock = Arc::new(AtomicU64::new(0));
            let polled = Arc::new(AtomicU64::new(0));
            let server = server::Server {
                mode: 0,
                current: 0,
                observe: server::api_impl::ObserveState::new(
                    LIMITS,
                    server::api_impl::ObserveBatches::new(ww_stream::batch::DEFAULT_POLICY),
                ),
                clock: clock.clone(),
                polled: polled.clone(),
                buf: vec![0; buf_len],
                scratch: Box::new(Scratch::new()),
            };
            let device = tests_common::start_device_async(
                path,
                server,
                observe_batched_api::OBSERVE_BATCHED_FULL_GID,
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
                        _ = updates_tx.send(Update {
                            path: path.iter().map(|p| p.unwrap().0).collect(),
                            t: t.map(us),
                            missed: missed.map_or(0, |m| m.0),
                            data: data.0.to_vec(),
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
            budget: Option<u32>,
        ) -> (Vec<ObserveEntry>, Vec<ObserveRefused>) {
            let path: Vec<UNib32> = path.iter().map(|p| UNib32(*p)).collect();
            let req = Request {
                seq: UVlq32Backfill(0),
                path_kind: PathKind::Absolute {
                    path: RefVec::Slice { slice: &path },
                },
                kind: RequestKind::Observe {
                    cfg: ObserveRequest {
                        entries: RefVec::Slice { slice: entries },
                        patch: false,
                        budget: budget.map(UNib32),
                    },
                },
            };
            let data = self.request(req).await;
            let granted = ObserveGranted::from_ww_bytes(&data).unwrap();
            (
                granted.entries.iter().map(|e| e.unwrap()).collect(),
                granted.refused.iter().map(|e| e.unwrap()).collect(),
            )
        }

        /// Call `sample(count, t0_us, dt_us, first)` on the device.
        async fn sample(&mut self, count: u32, t0_us: u32, dt_us: u32, first: i32) {
            self.client
                .sample(count, t0_us, dt_us, first)
                .call()
                .await
                .unwrap();
        }

        async fn request(&self, req: Request<'_>) -> Vec<u8> {
            let mut buf = [0u8; 512];
            let bytes = req.to_ww_bytes(&mut buf).unwrap().to_vec();
            let (done_tx, done_rx) = oneshot::channel();
            self.client
                .cmd
                .send(Command::SendMessage {
                    bytes,
                    done_tx: Some((done_tx, Duration::from_secs(2))),
                })
                .await
                .unwrap();
            done_rx.await.unwrap().expect("answer")
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

        /// Everything received until nothing came for 100 ms, in the order sent.
        async fn drain(&mut self) -> Vec<Update> {
            let mut out = vec![];
            while let Ok(Some(u)) =
                tokio::time::timeout(Duration::from_millis(100), self.updates.recv()).await
            {
                out.push(u);
            }
            out
        }
    }

    fn entry(index: u32, mode: ObserveMode) -> ObserveEntry {
        ObserveEntry {
            index: UNib32(index),
            mode,
        }
    }

    fn samples(every_us: u32, max_age_us: u32) -> ObserveMode {
        ObserveMode::Samples {
            every_us: UNib32(every_us),
            max_age_us: UNib32(max_age_us),
        }
    }

    /// A blocking server whose samplers run in another context (a thread here, an interrupt on an MCU): the
    /// batches are split, `ObserveState` holds the consumer half.
    mod split_server {
        use super::*;

        pub type State = api_impl::ObserveState<api_impl::ObserveBatchConsumers<'static>>;

        pub struct Server {
            pub mode: u8,
            pub observe: State,
        }

        impl Server {
            fn observe_state(&mut self) -> &mut State {
                &mut self.observe
            }

            fn get_current(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<i32, ()> {
                GetResult::Value(0)
            }

            fn get_reading(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Reading, ()> {
                GetResult::Value(reading(0))
            }

            fn changed_mode(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn get_slow_temp(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<f32, ()> {
                GetResult::Value(0.0)
            }

            fn sample(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _count: u32,
                _t0_us: u32,
                _dt_us: u32,
                _first: i32,
            ) -> RpcResult<()> {
                RpcResult::Ready(())
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                observe_batched_api :: ObserveBatched for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "mode=value_on_changed, _=get_set",
            );
        }
    }

    fn observe_request(entries: &[ObserveEntry]) -> Vec<u8> {
        let req = Request {
            seq: UVlq32Backfill(1),
            path_kind: PathKind::Absolute {
                path: RefVec::Slice { slice: &[] },
            },
            kind: RequestKind::Observe {
                cfg: ObserveRequest {
                    entries: RefVec::Slice { slice: entries },
                    patch: false,
                    budget: None,
                },
            },
        };
        let mut buf = [0u8; 128];
        req.to_ww_bytes(&mut buf).unwrap().to_vec()
    }

    /// The sampler pushes from its own thread while the event loop polls: every sample arrives once, in order.
    #[test]
    fn sampler_in_another_thread() {
        use split_server::api_impl::{ObserveBatchedSamples, ObserveBatchedUpdate, ObserveBatches};
        let batches: &'static mut ObserveBatches = Box::leak(Box::new(ObserveBatches::new(
            ww_stream::batch::DEFAULT_POLICY,
        )));
        let (samplers, consumers) = batches.split();
        let mut server = split_server::Server {
            mode: 0,
            observe: split_server::State::new(LIMITS, consumers),
        };
        let req = observe_request(&[entry(CURRENT, samples(0, 1_000))]);
        let mut scratch = [0u8; 256];
        let mut queue_buf = vec![0u8; 64 * 1024];
        let mut queue = wire_weaver::EventQueue::new(&mut queue_buf);
        server
            .process_request_bytes(&req, &mut scratch, &mut queue, ())
            .unwrap();
        queue.clear();

        const COUNT: i32 = 2_000;
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let producer = {
            let done = done.clone();
            let mut root = samplers.root;
            std::thread::spawn(move || {
                let mut lost = 0;
                for v in 0..COUNT {
                    let t = Second::micro(v as u32 * 10);
                    lost += root.sample(
                        t,
                        ObserveBatchedSamples {
                            current: v,
                            reading: reading(v),
                        },
                    );
                    if v % 4 == 0 {
                        std::thread::yield_now();
                    }
                }
                done.store(true, Ordering::Release);
                lost
            })
        };
        let mut got = vec![];
        let mut missed = 0u32;
        let mut buf = [0u8; 512];
        // batches of the default size here
        let mut sb = Box::new(Scratch::<
            { scratch_bytes::<Timed<Reading>>(split_server::api_impl::OBSERVE_BATCH) },
        >::new());
        let mut now = 0;
        let collect =
            |queue: &mut wire_weaver::EventQueue, got: &mut Vec<i32>, missed: &mut u32| {
                for e in queue.iter() {
                    let Ok(EventKind::PropertyUpdate {
                        data, missed: m, ..
                    }) = Event::from_ww_bytes(e).unwrap().result
                    else {
                        continue;
                    };
                    *missed += m.map_or(0, |m| m.0);
                    let u = ObserveBatchedUpdate::from_ww_bytes(data.0).unwrap();
                    if let Some(Batched::Series(s)) = u.current {
                        got.extend(s.iter().map(|i| i.value));
                    }
                }
                queue.clear();
            };
        loop {
            let finished = done.load(Ordering::Acquire);
            now += 500;
            server
                .poll_properties(&mut queue, (), now, &mut buf, &mut sb)
                .unwrap();
            collect(&mut queue, &mut got, &mut missed);
            if finished && now > 1_000_000 {
                break;
            }
            if finished {
                now += 100_000;
            }
        }
        let lost = producer.join().unwrap();
        assert!(missed >= lost as u32, "{missed} {lost}");
        assert_eq!(got.len() + missed as usize, COUNT as usize);
        assert!(got.windows(2).all(|w| w[0] < w[1]), "in order, once");
    }

    /// Every sample arrives once, in order, each with its own time, in batches of at most 8 sealed full or by
    /// age; updates that carry only batches have no head `t`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn every_sample_once_in_order() {
        let mut b = Bench::start("observe_batched/every", 512).await;
        let (granted, refused) = b
            .observe(
                &[],
                &[
                    entry(CURRENT, samples(0, 5_000)),
                    entry(READING, samples(0, 5_000)),
                ],
                None,
            )
            .await;
        assert!(refused.is_empty(), "{refused:?}");
        assert_eq!(granted.len(), 2);
        // the event loop polls between bursts of a batch or less: nothing is lost
        for k in 0..5u32 {
            b.sample(6, 1_000 + k * 240, 40, 100 + k as i32 * 6).await;
            b.at(1_000 + k as u64 * 1_000).await;
        }
        b.run(6_000, 12_000, 1_000).await;
        let updates = b.drain().await;
        let current: Vec<(Option<u64>, i32)> = updates.iter().flat_map(Update::current).collect();
        let expected: Vec<(Option<u64>, i32)> = (0..30)
            .map(|i| (Some(1_000 + i as u64 * 40), 100 + i))
            .collect();
        assert_eq!(current, expected);
        let readings: Vec<(u64, Reading)> = updates.iter().flat_map(Update::readings).collect();
        assert_eq!(readings.len(), 30);
        assert_eq!(readings[29], (1_000 + 29 * 40, reading(129)));
        for u in &updates {
            assert!(u.path.is_empty());
            assert_eq!(u.t, None, "only batches: no head t");
            assert_eq!(u.missed, 0);
        }
        // 30 samples, batches of at most 8: at least 4 of each
        assert!(updates.len() >= 4, "{}", updates.len());

        // a burst of 30 with no poll in between: the batcher has two stores of 8, a batch sealed while the other
        // still waits loses its samples, reported as missed
        b.sample(30, 20_000, 40, 0).await;
        b.run(20_000, 30_000, 1_000).await;
        let updates = b.drain().await;
        let got = updates.iter().flat_map(Update::current).count();
        let missed: u32 = updates.iter().map(|u| u.missed).sum();
        assert!(got < 30);
        // `missed` counts the samples of both properties
        assert_eq!(got as u32 * 2 + missed, 60, "{got} {missed}");
    }

    /// A batched property observed with another mode is one value at the update's `t`, from its getter; a plain
    /// property next to batches brings the head `t` back.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn one_values_and_head_t() {
        let mut b = Bench::start("observe_batched/one", 512).await;
        b.observe(
            &[],
            &[
                entry(
                    CURRENT,
                    ObserveMode::Periodic {
                        every_us: UNib32(2_000),
                    },
                ),
                entry(READING, samples(0, 1_000)),
                entry(
                    MODE,
                    ObserveMode::OnChange {
                        min_us: UNib32(100),
                    },
                ),
            ],
            None,
        )
        .await;
        b.sample(4, 500, 100, 7).await;
        b.run(1_000, 4_000, 1_000).await;
        let updates = b.drain().await;
        // `current` as single values: one right away, then every 2 ms, the last sample's value from the getter
        let ones: Vec<(Option<u64>, i32)> = updates.iter().flat_map(Update::current).collect();
        // (where the grid starts depends on the first poll after the request)
        assert!(ones.len() >= 2, "{ones:?}");
        assert!(
            ones.windows(2)
                .all(|w| w[1].0.unwrap() - w[0].0.unwrap() == 2_000),
            "{ones:?}"
        );
        assert_eq!(ones.last().unwrap().1, 10, "{ones:?}");
        let readings: Vec<_> = updates.iter().flat_map(Update::readings).collect();
        assert_eq!(readings.len(), 4);
        assert_eq!(readings[0], (500, reading(7)));
        for u in &updates {
            let has_one = !u.current().is_empty() || u.mode().is_some();
            assert_eq!(u.t.is_some(), has_one, "{u:?}");
        }
    }

    /// `every_us` decimates on the device's grid; the nested level has a sampler of its own.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn decimated_and_nested() {
        let mut b = Bench::start("observe_batched/decimated", 512).await;
        b.observe(&[], &[entry(CURRENT, samples(200, 2_000))], None)
            .await;
        b.observe(&[SLOW], &[entry(TEMP, samples(0, 2_000))], None)
            .await;
        // 40 µs apart: one in five
        b.sample(50, 0, 40, 0).await;
        b.run(1_000, 6_000, 1_000).await;
        let updates = b.drain().await;
        let current: Vec<i32> = updates
            .iter()
            .filter(|u| u.path.is_empty())
            .flat_map(Update::current)
            .map(|(_, v)| v)
            .collect();
        assert_eq!(current, (0..10).map(|i| i * 5).collect::<Vec<_>>());
        let temps: Vec<f32> = updates
            .iter()
            .filter(|u| u.path == [SLOW])
            .flat_map(Update::temps)
            .collect();
        assert_eq!(temps, [0.0, 5.0, 10.0, 15.0, 20.0]);
    }

    /// `Samples` is untrusted input: only `+batched` properties take it, the age is clamped; switching a
    /// property to another mode drops its waiting batches and the sampler stops taking its samples.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn validated_and_switched_off() {
        let mut b = Bench::start("observe_batched/validated", 512).await;
        let (granted, refused) = b
            .observe(
                &[],
                &[
                    entry(MODE, samples(0, 1_000)),
                    entry(CURRENT, samples(0, 1)),
                ],
                None,
            )
            .await;
        assert_eq!(
            refused,
            [ObserveRefused {
                index: UNib32(MODE),
                reason: ObserveRefusal::ModeNotSupported
            }]
        );
        assert_eq!(granted, [entry(CURRENT, samples(0, 1_000))]);
        b.sample(5, 0, 10, 0).await;
        b.observe(&[], &[entry(READING, samples(0, 1_000))], None)
            .await;
        b.sample(5, 50, 10, 5).await;
        b.run(1_000, 4_000, 1_000).await;
        let updates = b.drain().await;
        assert!(
            updates.iter().all(|u| u.current().is_empty()),
            "{updates:?}"
        );
        let readings: Vec<_> = updates.iter().flat_map(Update::readings).collect();
        assert_eq!(readings.len(), 5);
        assert_eq!(readings[0].1, reading(5));
    }

    /// A small outbox and a short budget: batches the link can't take are dropped oldest first and counted as
    /// missed, so what arrives and what is missed add up to what was sampled.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dropped_batches_are_missed() {
        let mut b = Bench::start("observe_batched/missed", 512).await;
        b.observe(
            &[],
            &[
                entry(CURRENT, samples(0, 1_000)),
                entry(READING, samples(0, 1_000)),
            ],
            Some(2_000),
        )
        .await;
        let mut first = 0;
        let mut t = 0u64;
        for _ in 0..10 {
            b.sample(64, t as u32, 10, first).await;
            first += 64;
            t += 1_000;
            b.at(t).await;
        }
        b.run(t + 100_000, t + 1_000_000, 100_000).await;
        let updates = b.drain().await;
        let current: Vec<i32> = updates
            .iter()
            .flat_map(Update::current)
            .map(|(_, v)| v)
            .collect();
        let readings = updates.iter().flat_map(Update::readings).count();
        let missed: u64 = updates.iter().map(|u| u.missed as u64).sum();
        assert!(missed > 0);
        assert_eq!(
            current.len() + readings + missed as usize,
            2 * 640,
            "{missed}"
        );
        // newest kept, in order, no duplicates
        assert!(current.windows(2).all(|w| w[0] < w[1]), "{current:?}");
    }

    /// Full microseconds of a time as the host gives it (unwrapped into a `U64`)
    fn full_us(t: Option<Second>) -> u64 {
        match t.map(|t| (t.prefix, t.value)) {
            Some((ww_si::Prefix::Micro, ww_si::ww_numeric::NumericValue::U64(t))) => t,
            t => panic!("timestamp {t:?}"),
        }
    }

    /// Feed `count` samples `dt_us` apart from `t0_us` in bursts of 6, polling between them.
    async fn feed(b: &mut Bench, count: u32, t0_us: u64, dt_us: u32, first: i32) -> u64 {
        let mut done = 0;
        let mut now = b.clock.load(Ordering::Relaxed);
        while done < count {
            let n = (count - done).min(6);
            b.sample(
                n,
                (t0_us + (done * dt_us) as u64) as u32,
                dt_us,
                first + done as i32,
            )
            .await;
            done += n;
            now += 1_000;
            b.at(now).await;
        }
        b.run(now + 1_000, now + 10_000, 1_000).await;
        now + 10_000
    }

    /// A property observer of a batched property gets one `Observed` per sample with its own device time,
    /// unwrapped across the wrap of the device's `u32` µs clock.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn host_gets_every_sample_with_its_time() {
        use wire_weaver_client::observe::samples;
        let mut b = Bench::start("observe_batched/host_samples", 512).await;
        let mut current = b
            .client
            .observe_current(samples(Duration::from_millis(5)))
            .subscribe()
            .await
            .unwrap();
        let wrap = 1u64 << 32;
        let t0 = wrap - 600;
        feed(&mut b, 30, t0, 40, 100).await;
        let mut got = vec![];
        while got.len() < 30 {
            let u = tokio::time::timeout(Duration::from_secs(2), current.recv())
                .await
                .expect("a sample in time")
                .unwrap();
            assert_eq!(u.missed, 0);
            got.push((full_us(u.t), u.value));
        }
        let expected: Vec<(u64, i32)> = (0..30).map(|i| (t0 + i as u64 * 40, 100 + i)).collect();
        assert_eq!(got, expected);
        assert!(got.last().unwrap().0 > wrap);
    }

    /// A level observer: `samples(max_age)` puts every batched property into `Samples` mode; the update's batched
    /// slots carry the samples with unwrapped times, the latest-value cache has the last one.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn host_observes_a_level_in_samples_mode() {
        use client::api_client::observe_batched_api_observe_batched::ObserveBatchedObserve;
        use wire_weaver_client::observe::{BatchedOwned, samples};
        let mut b = Bench::start("observe_batched/host_level", 512).await;
        let cfg = ObserveBatchedObserve::default().samples(Duration::from_millis(5));
        assert_eq!(cfg.current, Some(samples(Duration::from_millis(5))));
        assert_eq!(cfg.mode, None);
        let mut level = b.client.observe(cfg).subscribe().await.unwrap();
        assert!(level.granted().unwrap().refused.is_empty());
        feed(&mut b, 12, 2_000, 40, 0).await;
        let mut readings = vec![];
        while let Some(u) = level.try_recv().unwrap() {
            assert_eq!(u.t, None, "only batches");
            match u.value.reading {
                Some(BatchedOwned::Series(s)) => {
                    readings.extend(s.0.into_iter().map(|i| (full_us(Some(i.t)), i.value)))
                }
                None => {}
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(readings.len(), 12);
        assert_eq!(readings[11], (2_000 + 11 * 40, reading(11)));
        level.with_latest(|l| {
            let current = l.current.as_ref().unwrap();
            assert_eq!(current.value, 11);
            assert_eq!(full_us(current.t), 2_000 + 11 * 40);
        });
    }
}

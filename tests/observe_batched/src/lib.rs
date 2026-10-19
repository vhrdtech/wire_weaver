//! Batched observe end to end (API-39): `+observe+batched` properties observed in `Samples` mode, every sample the
//! device takes (or one per `every_us`) sent in batches, `Batched::Samples` slots next to `Batched::One` and plain
//! ones, the samples of a level sharing one time column (`sample_times`). The device runs the generated async server polled with a fake clock, the host the real event loop
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
    use ww_si::Second;
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
    /// Rows of the cells server's ring: what the default path's two stores hold
    const CELLS_ROWS: usize = 2 * N;

    /// Which server a test runs against: the root level on `<Level>Sampler::sample` or on observe cells
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Variant {
        Sampler,
        Cells,
    }
    /// Where batches are rebuilt while they are encoded, one column at a time: the biggest column type
    const SB: usize = scratch_for(N);

    /// Scratch bytes for batches of `n` rows: the biggest of the time column and the property types
    const fn scratch_for(n: usize) -> usize {
        let time = scratch_bytes::<Second>(n);
        let reading = scratch_bytes::<Reading>(n);
        if time > reading { time } else { reading }
    }

    const LIMITS: ObserveLimits = ObserveLimits {
        min_period_us: 100,
        default_budget: 100_000,
        max_level_budget: 1_000_000,
        total_budget: 2_000_000,
        timestamps: true,
        samples_max_age_us: (1_000, 1_000_000),
        samples_min_every_us: 10,
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

    /// The same server with the root level on observe cells (`observe_cells = "root"`, API-50): the values go
    /// into the cells' ring in place, `slow` stays on its sampler, so one device runs both paths.
    mod cells_server {
        use super::*;
        use tests_common::TestProcessEventsAsync;

        pub struct Server {
            pub mode: u8,
            /// Last sample taken
            pub current: i32,
            pub observe: api_impl::ObserveState<api_impl::ObserveBatches<'static>>,
            /// The root level's cells, shared with `ObserveBatches` through an `Arc`
            pub root_cells: Arc<api_impl::RootCells<CELLS_ROWS>>,
            /// Fake microsecond clock, set by the test
            pub clock: Arc<AtomicU64>,
            /// Last fake time `poll_properties` ran at
            pub polled: Arc<AtomicU64>,
            pub buf: Vec<u8>,
            pub scratch: Box<Scratch<SB>>,
        }

        impl Server {
            fn observe_state(
                &mut self,
            ) -> &mut api_impl::ObserveState<api_impl::ObserveBatches<'static>> {
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
                let mut w = self.root_cells.writer().unwrap();
                let mut samplers = self.observe.samplers();
                for i in 0..count {
                    let t = Second::micro(t0_us.wrapping_add(i * dt_us));
                    let v = first + i as i32;
                    w.current(v);
                    w.reading(reading(v));
                    w.commit(t);
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
                observe_cells = "root",
                // debug_to_file = "../../target/tests_observe_batched_cells_server.rs"
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
        /// The event's `sample_times`, µs as sent
        sample_times: Option<Vec<u64>>,
        missed: u32,
        data: Vec<u8>,
    }

    /// Samples of a `Batched::Samples` slot with their times from the event's `sample_times`, which has exactly as
    /// many
    fn with_times<T>(times: &Option<Vec<u64>>, values: Vec<T>) -> Vec<(u64, T)> {
        let times = times.as_ref().expect("sample_times next to samples");
        assert_eq!(times.len(), values.len(), "one time per sample");
        times.iter().copied().zip(values).collect()
    }

    impl Update {
        /// Samples of `current` in it: (t µs, value) per item of a series, or the update's t for a single value
        fn current(&self) -> Vec<(Option<u64>, i32)> {
            let u = ObserveBatchedUpdate::from_ww_bytes(&self.data).unwrap();
            match u.current {
                Some(Batched::One(v)) => vec![(self.t, v)],
                Some(Batched::Samples(s)) => with_times(&self.sample_times, s.iter().collect())
                    .into_iter()
                    .map(|(t, v)| (Some(t), v))
                    .collect(),
                None => vec![],
            }
        }

        fn readings(&self) -> Vec<(u64, Reading)> {
            let u = ObserveBatchedUpdate::from_ww_bytes(&self.data).unwrap();
            match u.reading {
                Some(Batched::Samples(s)) => with_times(&self.sample_times, s.iter().collect()),
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
                Some(Batched::Samples(s)) => with_times(&self.sample_times, s.iter().collect())
                    .into_iter()
                    .map(|(_, v)| v)
                    .collect(),
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
        async fn start(variant: Variant, path: &str, buf_len: usize) -> Bench {
            let _ = tracing_subscriber::fmt::try_init();
            let clock = Arc::new(AtomicU64::new(0));
            let polled = Arc::new(AtomicU64::new(0));
            let device = match variant {
                Variant::Sampler => {
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
                    tests_common::start_device_async(
                        path,
                        server,
                        observe_batched_api::OBSERVE_BATCHED_FULL_GID,
                        server::api_impl::api_hash(),
                    )
                }
                Variant::Cells => {
                    let root_cells = Arc::new(cells_server::api_impl::RootCells::new());
                    let reader = ww_stream::batch::CellsReader::shared(root_cells.clone()).unwrap();
                    let server = cells_server::Server {
                        mode: 0,
                        current: 0,
                        observe: cells_server::api_impl::ObserveState::new(
                            LIMITS,
                            cells_server::api_impl::ObserveBatches::new(
                                ww_stream::batch::DEFAULT_POLICY,
                                reader,
                            ),
                        ),
                        root_cells,
                        clock: clock.clone(),
                        polled: polled.clone(),
                        buf: vec![0; buf_len],
                        scratch: Box::new(Scratch::new()),
                    };
                    tests_common::start_device_async(
                        &format!("{path}/cells"),
                        server,
                        observe_batched_api::OBSERVE_BATCHED_FULL_GID,
                        cells_server::api_impl::api_hash(),
                    )
                }
            };
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
                        sample_times,
                        data,
                    }) = event.result
                    {
                        _ = updates_tx.send(Update {
                            path: path.iter().map(|p| p.unwrap().0).collect(),
                            t: t.map(us),
                            sample_times: sample_times.map(|s| s.iter().map(us).collect()),
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
            pub observe: &'static mut State,
        }

        impl Server {
            fn observe_state(&mut self) -> &mut State {
                self.observe
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
        // as a firmware sets up: the batches and the state built in place in their statics (leaked boxes here),
        // over garbage, so `init` has to write every field
        fn garbage<T>() -> &'static mut core::mem::MaybeUninit<T> {
            let slot = Box::leak(Box::new(core::mem::MaybeUninit::<T>::uninit()));
            // SAFETY: bytes of a `MaybeUninit`, any value is fine
            unsafe {
                slot.as_mut_ptr()
                    .cast::<u8>()
                    .write_bytes(0xA5, size_of::<T>())
            };
            slot
        }
        let batches = ObserveBatches::init(garbage(), ww_stream::batch::DEFAULT_POLICY);
        let (samplers, consumers) = batches.split();
        let mut server = split_server::Server {
            mode: 0,
            observe: split_server::State::init(garbage(), LIMITS, consumers),
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
            { scratch_for(split_server::api_impl::OBSERVE_BATCH) },
        >::new());
        let mut now = 0;
        let collect =
            |queue: &mut wire_weaver::EventQueue, got: &mut Vec<i32>, missed: &mut u32| {
                for e in queue.iter() {
                    let Ok(EventKind::PropertyUpdate {
                        data,
                        missed: m,
                        sample_times,
                        ..
                    }) = Event::from_ww_bytes(e).unwrap().result
                    else {
                        continue;
                    };
                    *missed += m.map_or(0, |m| m.0);
                    let u = ObserveBatchedUpdate::from_ww_bytes(data.0).unwrap();
                    if let Some(Batched::Samples(s)) = u.current {
                        let times = sample_times.expect("sample_times");
                        assert_eq!(times.iter().count(), s.iter().count());
                        got.extend(s.iter());
                    }
                }
                queue.clear();
            };
        // the event loop keeps polling for a second of device time after the producer is done, so what it
        // sealed or lost last gets out (the fake clock may be far ahead of the producer on a busy PC)
        let mut finished_at = None;
        loop {
            if finished_at.is_none() && done.load(Ordering::Acquire) {
                finished_at = Some(now);
            }
            now += 500;
            server
                .poll_properties(&mut queue, (), now, &mut buf, &mut sb)
                .unwrap();
            collect(&mut queue, &mut got, &mut missed);
            match finished_at {
                Some(at) if now > at + 1_000_000 => break,
                Some(_) => now += 100_000,
                None => {}
            }
        }
        let lost = producer.join().unwrap();
        assert!(missed >= lost as u32, "{missed} {lost}");
        assert_eq!(got.len() + missed as usize, COUNT as usize);
        assert!(got.windows(2).all(|w| w[0] < w[1]), "in order, once");
    }

    /// Every sample arrives once, in order, each with its own time, in batches of at most 8 sealed full or by
    /// age; updates that carry only batches have no head `t`.
    async fn every_sample_once_in_order(variant: Variant) {
        let mut b = Bench::start(variant, "observe_batched/every", 512).await;
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
    async fn one_values_and_head_t(variant: Variant) {
        let mut b = Bench::start(variant, "observe_batched/one", 512).await;
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
    async fn decimated_and_nested(variant: Variant) {
        let mut b = Bench::start(variant, "observe_batched/decimated", 512).await;
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

    /// `Samples` is untrusted input: only `+batched` properties take it, the age and grid are clamped; switching a
    /// property to another mode drops its column from the waiting batches. The rows of a level carry every value,
    /// so a property switched into `Samples` gets the rows not sent yet too, each at its time.
    async fn validated_and_switched_off(variant: Variant) {
        let mut b = Bench::start(variant, "observe_batched/validated", 512).await;
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
        // every sample asked for: the device's floor of one per 10 µs, the age raised to 1 ms
        assert_eq!(granted, [entry(CURRENT, samples(10, 1_000))]);
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
        let expected: Vec<(u64, Reading)> = (0..10).map(|i| (i * 10, reading(i as i32))).collect();
        assert_eq!(readings, expected);
    }

    /// The `Samples` entries of a level share one grid and one age: the finest `every_us` and the shortest
    /// `max_age_us` asked, reported in the grant; every update with samples has one time column for all of them.
    async fn level_wide_samples_settings(variant: Variant) {
        let mut b = Bench::start(variant, "observe_batched/level_wide", 512).await;
        let (granted, refused) = b
            .observe(
                &[],
                &[
                    entry(CURRENT, samples(400, 5_000)),
                    entry(READING, samples(200, 2_000)),
                ],
                None,
            )
            .await;
        assert!(refused.is_empty(), "{refused:?}");
        assert_eq!(
            granted,
            [
                entry(CURRENT, samples(200, 2_000)),
                entry(READING, samples(200, 2_000))
            ]
        );
        // 40 µs apart: one in five, for both
        b.sample(40, 0, 40, 0).await;
        b.run(1_000, 6_000, 1_000).await;
        let updates = b.drain().await;
        let current: Vec<(Option<u64>, i32)> = updates.iter().flat_map(Update::current).collect();
        let expected: Vec<(Option<u64>, i32)> =
            (0..8).map(|i| (Some(i as u64 * 200), i * 5)).collect();
        assert_eq!(current, expected);
        let readings: Vec<(u64, Reading)> = updates.iter().flat_map(Update::readings).collect();
        assert_eq!(
            readings,
            (0..8)
                .map(|i| (i as u64 * 200, reading(i * 5)))
                .collect::<Vec<_>>()
        );
        for u in &updates {
            // both columns in every update that has samples
            assert_eq!(u.current().len(), u.readings().len(), "{u:?}");
        }
    }

    /// A property turned off while its samples wait in the batcher: the batch goes out without its column, the
    /// other property's samples keep their times.
    async fn column_dropped_from_a_waiting_batch(variant: Variant) {
        let mut b = Bench::start(variant, "observe_batched/column_dropped", 512).await;
        b.observe(
            &[],
            &[
                entry(CURRENT, samples(0, 2_000)),
                entry(READING, samples(0, 2_000)),
            ],
            None,
        )
        .await;
        // fewer than a batch, and the clock stays: nothing is sealed yet
        b.sample(5, 100, 10, 0).await;
        b.observe(&[], &[entry(CURRENT, samples(0, 2_000))], None)
            .await;
        b.run(1_000, 5_000, 1_000).await;
        let updates = b.drain().await;
        assert!(
            updates.iter().all(|u| u.readings().is_empty()),
            "{updates:?}"
        );
        let current: Vec<(Option<u64>, i32)> = updates.iter().flat_map(Update::current).collect();
        let expected: Vec<(Option<u64>, i32)> =
            (0..5).map(|i| (Some(100 + i as u64 * 10), i)).collect();
        assert_eq!(current, expected);
    }

    /// A buffer that holds less than a batch of 8 rows: the batcher seals smaller batches at what fits (at the
    /// worst case of a row until it measured one batch), each sample arrives once with its time or is missed.
    async fn small_buffer_seals_smaller_batches(variant: Variant) {
        let mut b = Bench::start(variant, "observe_batched/small_buffer", 96).await;
        b.observe(
            &[],
            &[
                entry(CURRENT, samples(0, 5_000)),
                entry(READING, samples(0, 5_000)),
            ],
            None,
        )
        .await;
        // two samples between polls
        for k in 0..15u32 {
            b.sample(2, 1_000 + k * 80, 40, k as i32 * 2).await;
            b.at(1_000 + k as u64 * 1_000).await;
        }
        b.run(16_000, 22_000, 1_000).await;
        let updates = b.drain().await;
        let current: Vec<(Option<u64>, i32)> = updates.iter().flat_map(Update::current).collect();
        assert!(
            current
                .iter()
                .all(|(t, v)| *t == Some(1_000 + *v as u64 * 40)),
            "{current:?}"
        );
        assert!(current.windows(2).all(|w| w[0].1 < w[1].1), "{current:?}");
        let readings = updates.iter().flat_map(Update::readings).count();
        assert_eq!(readings, current.len());
        let missed: u32 = updates.iter().map(|u| u.missed).sum();
        assert_eq!(current.len() as u32 * 2 + missed, 60, "{missed}");
        assert!(current.len() >= 25, "{current:?}");
        // batches of fewer than 8 rows, more than one in some
        assert!(updates.len() > 30 / 8 + 1, "{}", updates.len());
        assert!(updates.iter().any(|u| u.current().len() > 1), "{updates:?}");
        assert!(updates.iter().all(|u| u.data.len() <= 96), "{updates:?}");
    }

    /// A small outbox and a short budget: batches the link can't take are dropped oldest first and counted as
    /// missed, so what arrives and what is missed add up to what was sampled.
    async fn dropped_batches_are_missed(variant: Variant) {
        let mut b = Bench::start(variant, "observe_batched/missed", 512).await;
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
    async fn host_gets_every_sample_with_its_time(variant: Variant) {
        use wire_weaver_client::observe::samples;
        let mut b = Bench::start(variant, "observe_batched/host_samples", 512).await;
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
    async fn host_observes_a_level_in_samples_mode(variant: Variant) {
        use client::api_client::observe_batched_api_observe_batched::ObserveBatchedObserve;
        use wire_weaver_client::observe::{BatchedOwned, samples};
        let mut b = Bench::start(variant, "observe_batched/host_level", 512).await;
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
                Some(BatchedOwned::Samples(s)) => {
                    // the level unwrapped the shared times
                    let times = u.sample_times.clone().expect("sample_times");
                    assert_eq!(times.len(), s.0.len());
                    readings.extend(
                        times
                            .into_iter()
                            .zip(s.0)
                            .map(|(t, v)| (full_us(Some(t)), v)),
                    );
                    // what the level observer pairs them with
                    assert!(
                        BatchedOwned::Samples(Vec::<i32>::new().into())
                            .into_samples(None, u.sample_times.as_deref())
                            .is_err()
                    );
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

    /// The dynamic client (TOOL-11) decodes `Batched` slots from introspection alone: every sample of each
    /// property with its time unwrapped by the level across the `u32` µs wrap, the newest in `values`.
    async fn dynamic_observer_gets_every_sample(variant: Variant) {
        use std::collections::BTreeMap;
        use wire_weaver_client::DynResource;
        use wire_weaver_client::observe::samples;
        use wire_weaver_client::ww_numeric::NumericValue;
        use wire_weaver_client::ww_self::{FieldsValueOwned, ValueOwned};
        let mut b = Bench::start(variant, "observe_batched/dynamic", 512).await;
        let bundle = b
            .client
            .cmd
            .device_introspect()
            .expect("introspection enabled")
            .api_bundle
            .clone();
        let root = DynResource::root(bundle);
        let mode = samples(Duration::from_millis(5));
        let entries = BTreeMap::from([(CURRENT, mode), (READING, mode)]);
        let mut observer = root.observe(&b.client.cmd, entries).await.unwrap();
        assert!(observer.granted().unwrap().refused.is_empty());
        let wrap = 1u64 << 32;
        let t0 = wrap - 600;
        feed(&mut b, 30, t0, 40, 100).await;
        let mut current = vec![];
        let mut readings = vec![];
        while current.len() < 30 {
            let u = tokio::time::timeout(Duration::from_secs(2), observer.recv())
                .await
                .expect("an update in time")
                .unwrap();
            assert_eq!(u.t, None, "only batches");
            assert_eq!(u.missed, 0);
            for s in &u.samples {
                let newest = u.values.iter().find(|(name, _)| *name == s.name);
                assert_eq!(newest.map(|(_, v)| v), s.items.last().map(|(v, _)| v));
                for (value, t) in &s.items {
                    let t = full_us(*t);
                    match (s.name.as_str(), value) {
                        ("current", ValueOwned::Numeric(NumericValue::I32(v))) => {
                            current.push((t, *v))
                        }
                        ("reading", ValueOwned::Struct { fields }) => {
                            let FieldsValueOwned::Named(fields) = fields else {
                                panic!("{fields:?}")
                            };
                            readings.push((t, fields[0].1.clone()));
                        }
                        other => panic!("{other:?}"),
                    }
                }
            }
        }
        let expected: Vec<(u64, i32)> = (0..30).map(|i| (t0 + i as u64 * 40, 100 + i)).collect();
        assert_eq!(current, expected);
        assert_eq!(readings.len(), 30);
        assert_eq!(
            readings[29],
            (
                t0 + 29 * 40,
                ValueOwned::Numeric(NumericValue::I32(2 * 129))
            )
        );
    }

    /// Events an async server sent, collected in memory.
    #[derive(Default)]
    struct Collect(Vec<Vec<u8>>);

    impl wire_weaver::MessageSink for Collect {
        async fn send(&mut self, message: &[u8]) -> Result<(), ()> {
            self.0.push(message.to_vec());
            Ok(())
        }
    }

    /// The `current` samples in the updates `out` collected, oldest first; empties it.
    fn current_samples(out: &mut wire_weaver::EventWriter<'_, Collect>) -> Vec<i32> {
        let mut got = vec![];
        for e in core::mem::take(&mut out.sink.0).iter() {
            let Ok(EventKind::PropertyUpdate { data, .. }) =
                Event::from_ww_bytes(e).unwrap().result
            else {
                continue;
            };
            let u = server::api_impl::ObserveBatchedUpdate::from_ww_bytes(data.0).unwrap();
            if let Some(Batched::Samples(s)) = u.current {
                got.extend(s.iter());
            }
        }
        got
    }

    /// A reset (the link went down) drops the rows taken before it: the next observer gets only what is sampled
    /// after it asked, not the rows still waiting in a batch or the cells' ring, with their old times.
    #[tokio::test]
    async fn reset_drops_waiting_rows() {
        let req = observe_request(&[entry(CURRENT, samples(0, 5_000))]);
        let mut scratch = [0u8; 512];
        let mut event_scratch = [0u8; 1024];
        let mut queue = wire_weaver::EventWriter::new(Collect::default(), &mut event_scratch);
        let mut buf = [0u8; 512];
        let mut sb = Box::new(Scratch::<SB>::new());
        let sample_rows = |first: i32, t0: u32| {
            (0..3).map(move |i| (Second::micro(t0 + i as u32 * 100), first + i))
        };

        let mut s = server::Server {
            mode: 0,
            current: 0,
            observe: server::api_impl::ObserveState::new(
                LIMITS,
                server::api_impl::ObserveBatches::new(ww_stream::batch::DEFAULT_POLICY),
            ),
            clock: Arc::new(AtomicU64::new(0)),
            polled: Arc::new(AtomicU64::new(0)),
            buf: vec![],
            scratch: Box::new(Scratch::new()),
        };
        s.process_request_bytes(&req, &mut scratch, &mut queue, ())
            .await
            .unwrap();
        queue.sink.0.clear();
        for (t, v) in sample_rows(1, 100) {
            let row = server::api_impl::ObserveBatchedSamples {
                current: v,
                reading: reading(v),
            };
            s.observe.samplers().root.sample(t, row);
        }
        s.observe.reset();
        s.process_request_bytes(&req, &mut scratch, &mut queue, ())
            .await
            .unwrap();
        queue.sink.0.clear();
        for (t, v) in sample_rows(100, 10_000) {
            let row = server::api_impl::ObserveBatchedSamples {
                current: v,
                reading: reading(v),
            };
            s.observe.samplers().root.sample(t, row);
        }
        for now in (20_000..=60_000).step_by(10_000) {
            s.poll_properties(&mut queue, (), now, &mut buf, &mut sb)
                .await
                .unwrap();
        }
        assert_eq!(current_samples(&mut queue), [100, 101, 102], "sampler");

        let root_cells = Arc::new(cells_server::api_impl::RootCells::<CELLS_ROWS>::new());
        let reader = ww_stream::batch::CellsReader::shared(root_cells.clone()).unwrap();
        let mut c = cells_server::Server {
            mode: 0,
            current: 0,
            observe: cells_server::api_impl::ObserveState::new(
                LIMITS,
                cells_server::api_impl::ObserveBatches::new(
                    ww_stream::batch::DEFAULT_POLICY,
                    reader,
                ),
            ),
            root_cells: root_cells.clone(),
            clock: Arc::new(AtomicU64::new(0)),
            polled: Arc::new(AtomicU64::new(0)),
            buf: vec![],
            scratch: Box::new(Scratch::new()),
        };
        let mut w = root_cells.writer().unwrap();
        c.process_request_bytes(&req, &mut scratch, &mut queue, ())
            .await
            .unwrap();
        queue.sink.0.clear();
        for (t, v) in sample_rows(1, 100) {
            w.current(v);
            w.reading(reading(v));
            w.commit(t);
        }
        c.observe.reset();
        c.process_request_bytes(&req, &mut scratch, &mut queue, ())
            .await
            .unwrap();
        queue.sink.0.clear();
        for (t, v) in sample_rows(100, 10_000) {
            w.current(v);
            w.reading(reading(v));
            w.commit(t);
        }
        for now in (20_000..=60_000).step_by(10_000) {
            c.poll_properties(&mut queue, (), now, &mut buf, &mut sb)
                .await
                .unwrap();
        }
        assert_eq!(current_samples(&mut queue), [100, 101, 102], "cells");
    }

    /// Event-driven (SRV-25): the sampler and the cells writer wake the server task on a batch's first row,
    /// `next_wake` says now until a poll has seen it, then its age, and nothing once it is sent.
    #[tokio::test]
    async fn batches_wake_and_next_wake() {
        static SIG_S: server::api_impl::ObserveSignal = server::api_impl::ObserveSignal::new();
        static SIG_C: cells_server::api_impl::ObserveSignal =
            cells_server::api_impl::ObserveSignal::new();
        let req = observe_request(&[entry(CURRENT, samples(0, 5_000))]);
        let mut scratch = [0u8; 512];
        let mut event_scratch = [0u8; 1024];
        let mut queue = wire_weaver::EventWriter::new(Collect::default(), &mut event_scratch);
        let mut buf = [0u8; 512];
        let mut sb = Box::new(Scratch::<SB>::new());

        let mut s = server::Server {
            mode: 0,
            current: 0,
            observe: server::api_impl::ObserveState::new(
                LIMITS,
                server::api_impl::ObserveBatches::new(ww_stream::batch::DEFAULT_POLICY),
            ),
            clock: Arc::new(AtomicU64::new(0)),
            polled: Arc::new(AtomicU64::new(0)),
            buf: vec![],
            scratch: Box::new(Scratch::new()),
        };
        s.observe.with_signal(&SIG_S);
        s.process_request_bytes(&req, &mut scratch, &mut queue, ())
            .await
            .unwrap();
        assert_eq!(s.observe.next_wake(0), Some(0));
        s.poll_properties(&mut queue, (), 0, &mut buf, &mut sb)
            .await
            .unwrap();
        queue.sink.0.clear();
        assert_eq!(s.observe.next_wake(0), None);
        for (i, t) in [100u32, 200, 300].into_iter().enumerate() {
            let row = server::api_impl::ObserveBatchedSamples {
                current: i as i32,
                reading: reading(i as i32),
            };
            s.observe
                .samplers()
                .root
                .with_wake(SIG_S.as_wake())
                .sample(Second::micro(t), row);
            // the first row wakes, the others don't
            assert_eq!(SIG_S.as_wake().take(), i == 0, "row {i}");
            if i == 0 {
                assert_eq!(s.observe.next_wake(1_000), Some(1_000));
                s.poll_properties(&mut queue, (), 1_000, &mut buf, &mut sb)
                    .await
                    .unwrap();
                assert_eq!(s.observe.next_wake(1_000), Some(6_000), "its age");
            }
        }
        s.poll_properties(&mut queue, (), 6_000, &mut buf, &mut sb)
            .await
            .unwrap();
        assert_eq!(current_samples(&mut queue), [0, 1, 2], "sampler");
        assert_eq!(s.observe.next_wake(6_000), None);

        let root_cells = Arc::new(cells_server::api_impl::RootCells::<CELLS_ROWS>::new());
        let reader = ww_stream::batch::CellsReader::shared(root_cells.clone()).unwrap();
        let mut c = cells_server::Server {
            mode: 0,
            current: 0,
            observe: cells_server::api_impl::ObserveState::new(
                LIMITS,
                cells_server::api_impl::ObserveBatches::new(
                    ww_stream::batch::DEFAULT_POLICY,
                    reader,
                ),
            ),
            root_cells: root_cells.clone(),
            clock: Arc::new(AtomicU64::new(0)),
            polled: Arc::new(AtomicU64::new(0)),
            buf: vec![],
            scratch: Box::new(Scratch::new()),
        };
        c.observe.with_signal(&SIG_C);
        let mut w = root_cells.writer().unwrap().with_wake(SIG_C.as_wake());
        c.process_request_bytes(&req, &mut scratch, &mut queue, ())
            .await
            .unwrap();
        c.poll_properties(&mut queue, (), 0, &mut buf, &mut sb)
            .await
            .unwrap();
        queue.sink.0.clear();
        assert_eq!(c.observe.next_wake(0), None);
        for (i, t) in [100u32, 200, 300].into_iter().enumerate() {
            w.current(i as i32);
            w.reading(reading(i as i32));
            w.commit(Second::micro(t));
            assert_eq!(SIG_C.as_wake().take(), i == 0, "row {i}");
            if i == 0 {
                assert_eq!(c.observe.next_wake(1_000), Some(1_000));
                c.poll_properties(&mut queue, (), 1_000, &mut buf, &mut sb)
                    .await
                    .unwrap();
                assert_eq!(c.observe.next_wake(1_000), Some(6_000), "its age");
            }
        }
        c.poll_properties(&mut queue, (), 6_000, &mut buf, &mut sb)
            .await
            .unwrap();
        assert_eq!(current_samples(&mut queue), [0, 1, 2], "cells");
        assert_eq!(c.observe.next_wake(6_000), None);
    }

    /// The host can't tell the two apart: the same samples, sealed at the same rows (a full batch per poll), give
    /// byte for byte the same updates from a sampler level and a cells level.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cells_send_the_samplers_bytes() {
        let mut got = vec![];
        for variant in [Variant::Sampler, Variant::Cells] {
            let mut b = Bench::start(variant, "observe_batched/same_bytes", 512).await;
            b.observe(
                &[],
                &[
                    entry(CURRENT, samples(0, 1_000_000)),
                    entry(READING, samples(0, 1_000_000)),
                ],
                None,
            )
            .await;
            let mut t = 0u64;
            for k in 0..6 {
                b.sample(N as u32, (k * 400) as u32, 50, k * 100).await;
                t += 1_000;
                b.at(t).await;
            }
            let updates: Vec<_> = b
                .drain()
                .await
                .into_iter()
                .map(|u| (u.path, u.sample_times, u.missed, u.data))
                .collect();
            assert_eq!(updates.len(), 6, "{variant:?}");
            got.push(updates);
        }
        assert_eq!(got[0], got[1]);
    }

    // Each test against both servers: `<name>_sampler` on the sampler, `<name>_cells` on observe cells.

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn every_sample_once_in_order_sampler() {
        every_sample_once_in_order(Variant::Sampler).await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn every_sample_once_in_order_cells() {
        every_sample_once_in_order(Variant::Cells).await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn one_values_and_head_t_sampler() {
        one_values_and_head_t(Variant::Sampler).await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn one_values_and_head_t_cells() {
        one_values_and_head_t(Variant::Cells).await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn decimated_and_nested_sampler() {
        decimated_and_nested(Variant::Sampler).await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn decimated_and_nested_cells() {
        decimated_and_nested(Variant::Cells).await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn validated_and_switched_off_sampler() {
        validated_and_switched_off(Variant::Sampler).await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn validated_and_switched_off_cells() {
        validated_and_switched_off(Variant::Cells).await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn level_wide_samples_settings_sampler() {
        level_wide_samples_settings(Variant::Sampler).await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn level_wide_samples_settings_cells() {
        level_wide_samples_settings(Variant::Cells).await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn column_dropped_from_a_waiting_batch_sampler() {
        column_dropped_from_a_waiting_batch(Variant::Sampler).await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn column_dropped_from_a_waiting_batch_cells() {
        column_dropped_from_a_waiting_batch(Variant::Cells).await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn small_buffer_seals_smaller_batches_sampler() {
        small_buffer_seals_smaller_batches(Variant::Sampler).await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn small_buffer_seals_smaller_batches_cells() {
        small_buffer_seals_smaller_batches(Variant::Cells).await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dropped_batches_are_missed_sampler() {
        dropped_batches_are_missed(Variant::Sampler).await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dropped_batches_are_missed_cells() {
        dropped_batches_are_missed(Variant::Cells).await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn host_gets_every_sample_with_its_time_sampler() {
        host_gets_every_sample_with_its_time(Variant::Sampler).await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn host_gets_every_sample_with_its_time_cells() {
        host_gets_every_sample_with_its_time(Variant::Cells).await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn host_observes_a_level_in_samples_mode_sampler() {
        host_observes_a_level_in_samples_mode(Variant::Sampler).await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn host_observes_a_level_in_samples_mode_cells() {
        host_observes_a_level_in_samples_mode(Variant::Cells).await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dynamic_observer_gets_every_sample_sampler() {
        dynamic_observer_gets_every_sample(Variant::Sampler).await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dynamic_observer_gets_every_sample_cells() {
        dynamic_observer_gets_every_sample(Variant::Cells).await
    }
}

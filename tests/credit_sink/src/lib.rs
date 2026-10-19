//! Credit-based flow control for sinks (API-28) end to end: generated server on `ww_device::Server` in a device
//! thread, generated and dynamic clients through the real host event loop, see `docs/api/streams.md`.

#[cfg(test)]
mod tests {
    use credit_sink_api::Point;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver_client::{CreditConfig, Error, StreamError};
    use ww_stream::sink::SinkCredit;
    use ww_client_server::{Request, RequestKind, StreamSideband};

    /// Device behaviour and what it received, shared with the test.
    #[derive(Default)]
    struct Model {
        /// Unconsumed `plan` items, like a receive buffer
        buffer: Vec<Vec<u8>>,
        /// Most bytes ever buffered
        max_buffered: usize,
        /// Consumed `plan` bytes, in order
        plan: Vec<u8>,
        /// Consume the buffer once it holds this many bytes (a flash write), 0 consumes every write right away.
        /// At most `capacity - largest item + 1`, or the host can't send the next item and the device waits for it.
        drain_at: usize,
        /// How long a drain takes
        stall: Duration,
        /// How long each write takes
        per_write: Duration,
        /// Throw away this many re-grants after a drain, as if they were lost on the way
        drop_grants: u32,
        overruns: u32,
        points: Vec<Point>,
        lanes: [Vec<u8>; 4],
        plain: Vec<Vec<u8>>,
        blocks: Vec<Vec<u8>>,
        closed: u32,
        /// Units of each `points` write, as the host named them
        point_units: Vec<u64>,
        /// Forget the `plan` session on the next write, as a reboot would
        reboot: bool,
    }

    struct Config {
        capacity: u32,
        threshold: u32,
        model: Model,
    }

    impl Config {
        fn new(capacity: u32, threshold: u32) -> Self {
            Config {
                capacity,
                threshold,
                model: Model::default(),
            }
        }

        fn drain_at(mut self, bytes: usize, stall: Duration) -> Self {
            self.model.drain_at = bytes;
            self.model.stall = stall;
            self
        }
    }

    mod sync_server {
        use super::*;
        use tests_common::TestProcessEvents;

        pub struct Server {
            pub m: Arc<Mutex<Model>>,
            pub blocks_credit: SinkCredit,
            pub plan_credit: SinkCredit,
            pub points_credit: SinkCredit,
            pub lanes_credit: [SinkCredit; 4],
        }

        impl Server {
            fn credit_plan(&mut self) -> &mut SinkCredit {
                &mut self.plan_credit
            }

            fn sideband_plan(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                sideband: StreamSideband,
            ) -> Option<StreamSideband> {
                assert!(
                    !matches!(sideband, StreamSideband::Query { .. }),
                    "answered by generated code"
                );
                if sideband == StreamSideband::Close {
                    let mut m = self.m.lock().unwrap();
                    let rest = std::mem::take(&mut m.buffer).concat();
                    self.plan_credit.free(rest.len() as u32);
                    m.plan.extend(rest);
                    m.closed += 1;
                }
                None
            }

            fn write_plan(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                data: &shrink_wrap::tail_bytes::TailBytes<'_>,
            ) {
                let mut m = self.m.lock().unwrap();
                std::thread::sleep(m.per_write);
                if std::mem::take(&mut m.reboot) {
                    self.plan_credit = SinkCredit::new(self.plan_credit.capacity());
                    m.buffer.clear();
                    return;
                }
                m.buffer.push(data.0.to_vec());
                let buffered = m.buffer.iter().map(Vec::len).sum::<usize>();
                assert_eq!(buffered, self.plan_credit.buffered() as usize);
                m.max_buffered = m.max_buffered.max(buffered);
                if buffered >= m.drain_at {
                    std::thread::sleep(m.stall);
                    let consumed = std::mem::take(&mut m.buffer).concat();
                    self.plan_credit.free(consumed.len() as u32);
                    m.plan.extend(consumed);
                    if m.drop_grants > 0 && self.plan_credit.take_ack().is_some() {
                        m.drop_grants -= 1;
                    }
                }
                m.overruns = self.plan_credit.overruns();
            }

            fn credit_points(&mut self) -> &mut SinkCredit {
                &mut self.points_credit
            }

            fn credit_blocks(&mut self) -> &mut SinkCredit {
                &mut self.blocks_credit
            }

            fn sideband_blocks(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: StreamSideband,
            ) -> Option<StreamSideband> {
                None
            }

            fn write_blocks(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                data: &shrink_wrap::tail_bytes::TailBytes<'_>,
            ) {
                self.m.lock().unwrap().blocks.push(data.0.to_vec());
                self.blocks_credit.free(data.0.len() as u32);
            }

            fn sideband_points(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: StreamSideband,
            ) -> Option<StreamSideband> {
                None
            }

            fn write_points(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>, value: Point) {
                let mut m = self.m.lock().unwrap();
                m.points.push(value);
                m.point_units.push(self.points_credit.last_item_units());
                // a `sized` type always takes its largest size
                self.points_credit.free(POINT_BYTES);
            }

            fn valid_indices_root_lanes(&mut self) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..4)
            }

            fn credit_lanes(&mut self, index_chain: [UNib32; 1]) -> &mut SinkCredit {
                &mut self.lanes_credit[index_chain[0].0 as usize]
            }

            fn sideband_lanes(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _index_chain: [UNib32; 1],
                _sideband: StreamSideband,
            ) -> Option<StreamSideband> {
                None
            }

            fn write_lanes(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
                data: &shrink_wrap::tail_bytes::TailBytes<'_>,
            ) {
                let idx = index_chain[0].0 as usize;
                self.m.lock().unwrap().lanes[idx].extend_from_slice(data.0);
                self.lanes_credit[idx].free(data.0.len() as u32);
            }

            fn sideband_plain(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: StreamSideband,
            ) -> Option<StreamSideband> {
                None
            }

            fn write_plain(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                data: &shrink_wrap::tail_bytes::TailBytes<'_>,
            ) {
                self.m.lock().unwrap().plain.push(data.0.to_vec());
            }

            fn finish(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
                Ready(())
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                credit_sink_api :: CreditSinks for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                // debug_to_file = "../../target/tests_credit_sink_server.rs"
            );
        }

        impl TestProcessEvents for Server {
            type Medium = ();

            fn process_request_bytes<'a>(
                &mut self,
                bytes: &[u8],
                scratch: &'a mut [u8],
                out: &mut impl BlockingEventOut,
                medium: (),
            ) -> Result<&'a [u8], ShrinkWrapError> {
                self.process_request_bytes(bytes, scratch, out, medium)
            }
        }
    }

    /// Same API on an async server, consuming everything right away.
    mod async_server {
        use super::*;
        use tests_common::TestProcessEventsAsync;

        pub struct Server {
            pub m: Arc<Mutex<Model>>,
            pub credit: SinkCredit,
            pub other: SinkCredit,
        }

        impl Server {
            fn credit_plan(&mut self) -> &mut SinkCredit {
                &mut self.credit
            }

            async fn sideband_plan(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                _sideband: StreamSideband,
            ) -> Option<StreamSideband> {
                None
            }

            async fn write_plan(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                data: &shrink_wrap::tail_bytes::TailBytes<'_>,
            ) {
                self.m.lock().unwrap().plan.extend_from_slice(data.0);
                self.credit.free(data.0.len() as u32);
            }

            fn credit_points(&mut self) -> &mut SinkCredit {
                &mut self.other
            }

            fn credit_blocks(&mut self) -> &mut SinkCredit {
                &mut self.other
            }

            async fn sideband_blocks(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                _sideband: StreamSideband,
            ) -> Option<StreamSideband> {
                None
            }

            async fn write_blocks(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                _data: &shrink_wrap::tail_bytes::TailBytes<'_>,
            ) {
            }

            async fn sideband_points(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                _sideband: StreamSideband,
            ) -> Option<StreamSideband> {
                None
            }

            async fn write_points(&mut self, _cx: &mut Context<'_, impl EventOut>, _value: Point) {}

            fn valid_indices_root_lanes(&mut self) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..0)
            }

            fn credit_lanes(&mut self, _index_chain: [UNib32; 1]) -> &mut SinkCredit {
                &mut self.other
            }

            async fn sideband_lanes(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                _index_chain: [UNib32; 1],
                _sideband: StreamSideband,
            ) -> Option<StreamSideband> {
                None
            }

            async fn write_lanes(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                _index_chain: [UNib32; 1],
                _data: &shrink_wrap::tail_bytes::TailBytes<'_>,
            ) {
            }

            async fn sideband_plain(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                _sideband: StreamSideband,
            ) -> Option<StreamSideband> {
                None
            }

            async fn write_plain(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                _data: &shrink_wrap::tail_bytes::TailBytes<'_>,
            ) {
            }

            async fn finish(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
                Ready(())
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                credit_sink_api :: CreditSinks for super::Server,
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

            fn from_cmd(cmd: Commander) -> Self {
                Self { cmd }
            }
        }

        mod api_client {
            wire_weaver::ww_codegen!(
                credit_sink_api :: CreditSinks for super::Client,
                client = "std_client",
            );
        }
    }

    use client::Client;
    use sync_server::api_impl::CreditSinkApiCreditSinksStreamSerializer as Ser;

    const POINT_BYTES: u32 = Ser::POINTS_MAX_ITEM_BYTES.unwrap();

    fn start(path: &str, config: Config) -> (TestDevice, Arc<Mutex<Model>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let m = Arc::new(Mutex::new(config.model));
        let credit = || SinkCredit::new(config.capacity).with_threshold(config.threshold);
        let server = sync_server::Server {
            m: m.clone(),
            plan_credit: credit(),
            blocks_credit: SinkCredit::sized_for(8, Ser::BLOCKS_MAX_ITEM_BYTES).with_threshold(4),
            // 4 points
            points_credit: SinkCredit::sized_for(16, Ser::POINTS_MAX_ITEM_BYTES).with_threshold(8),
            lanes_credit: [(); 4].map(|_| SinkCredit::new(32).with_threshold(16)),
        };
        let device = tests_common::start_device(
            path,
            server,
            credit_sink_api::CREDIT_SINKS_FULL_GID,
            sync_server::api_impl::api_hash(),
        );
        (device, m)
    }

    async fn connect(device: &TestDevice) -> Client {
        Client::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    fn data(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 7 + i / 251) as u8).collect()
    }

    /// Position of a write to a credit sink, None for other requests
    fn write_pos(request: &[u8]) -> Option<u32> {
        let mut rd = BufReader::new(request);
        let request = Request::des_shrink_wrap(&mut rd).ok()?;
        request.kind.as_sink_write().map(|(_, pos, _)| pos)
    }

    /// A credit sink's open: `StreamOpen` with the host's session
    fn is_open(request: &[u8]) -> bool {
        let mut rd = BufReader::new(request);
        Request::des_shrink_wrap(&mut rd)
            .is_ok_and(|request| matches!(request.kind, RequestKind::StreamOpen { resume: Some(_), .. }))
    }

    fn sideband_of(request: &[u8]) -> Option<StreamSideband> {
        let mut rd = BufReader::new(request);
        let request = Request::des_shrink_wrap(&mut rd).ok()?;
        match request.kind {
            RequestKind::StreamSideband { sideband } => Some(sideband),
            _ => None,
        }
    }

    /// Wait for the device thread to process what was sent to it
    async fn eventually(m: &Arc<Mutex<Model>>, f: impl Fn(&Model) -> bool) {
        for _ in 0..2000 {
            if f(&m.lock().unwrap()) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        panic!("condition not met within 2 s");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn flush_waits_until_everything_is_accepted() {
        let (device, m) = start(
            "credit/flush",
            Config::new(256, 128).drain_at(0, Duration::ZERO),
        );
        let client = connect(&device).await;
        let mut plan = client.plan().await.unwrap();
        assert!(plan.is_credit());
        let bytes = data(1000);
        for chunk in bytes.chunks(100) {
            plan.send_bytes(chunk).await.unwrap();
        }
        plan.flush().await.unwrap();
        // no waiting for the device here: flush returned after it accepted everything
        assert_eq!(m.lock().unwrap().plan, bytes);
        assert_eq!(plan.credit_progress(), Some((1000, 1000)));
        plan.close().await.unwrap();
        eventually(&m, |m| m.closed == 1).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn slow_device_is_never_overrun() {
        // a buffer of 1 KiB, written to flash once it can't take another item of up to 300 bytes
        let config = Config::new(1024, 256).drain_at(1024 - 300 + 1, Duration::from_millis(20));
        let (device, m) = start("credit/slow", config);
        m.lock().unwrap().per_write = Duration::from_millis(1);
        let client = connect(&device).await;
        let mut plan = client.plan().await.unwrap();
        let bytes = data(16 * 1024);
        let mut rest = &bytes[..];
        for len in [1, 300, 37, 128, 5, 299, 64, 2].into_iter().cycle() {
            if rest.is_empty() {
                break;
            }
            let (item, tail) = rest.split_at(len.min(rest.len()));
            plan.send_bytes(item).await.unwrap();
            rest = tail;
        }
        plan.close().await.unwrap();
        eventually(&m, |m| m.closed == 1).await;
        let m = m.lock().unwrap();
        assert_eq!(m.plan, bytes);
        assert!(m.max_buffered <= 1024, "{}", m.max_buffered);
        assert!(m.max_buffered > 1024 - 300, "{}", m.max_buffered);
        assert_eq!(m.overruns, 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn throughput() {
        let config = Config::new(8192, 4096).drain_at(4096, Duration::ZERO);
        let (device, m) = start("credit/throughput", config);
        let client = connect(&device).await;
        let mut plan = client.plan().await.unwrap();
        let bytes = data(512 * 1024);
        let start = Instant::now();
        for chunk in bytes.chunks(512) {
            plan.send_bytes(chunk).await.unwrap();
        }
        plan.flush().await.unwrap();
        let elapsed = start.elapsed();
        println!(
            "credit sink: {} KiB in {elapsed:?}, {:.0} KiB/s",
            bytes.len() / 1024,
            bytes.len() as f64 / 1024.0 / elapsed.as_secs_f64()
        );
        plan.close().await.unwrap();
        eventually(&m, |m| m.closed == 1).await;
        let m = m.lock().unwrap();
        assert_eq!(m.plan, bytes);
        assert_eq!(m.overruns, 0);
        assert!(m.max_buffered <= 8192);
        assert!(elapsed < Duration::from_secs(10), "{elapsed:?}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn lost_credit_event_is_recovered_by_a_query() {
        // one re-grant per drain, and the first one is lost
        let config = Config::new(512, 512).drain_at(512, Duration::ZERO);
        let (device, m) = start("credit/lost_credit", config);
        m.lock().unwrap().drop_grants = 1;
        let queries = Arc::new(Mutex::new(0));
        let q = queries.clone();
        device.filter_requests(move |request| {
            if let Some(StreamSideband::Query { .. }) = sideband_of(request) {
                *q.lock().unwrap() += 1;
            }
            false
        });
        let client = connect(&device).await;
        let mut plan = client.plan().await.unwrap();
        plan.set_credit_config(CreditConfig {
            query_interval: Duration::from_millis(50),
            stall_timeout: Some(Duration::from_secs(2)),
        });
        let bytes = data(4096);
        let start = Instant::now();
        for chunk in bytes.chunks(128) {
            plan.send_bytes(chunk).await.unwrap();
        }
        plan.close().await.unwrap();
        eventually(&m, |m| m.closed == 1).await;
        assert_eq!(m.lock().unwrap().plan, bytes);
        assert_eq!(m.lock().unwrap().drop_grants, 0, "a grant was dropped");
        // one query to recover from the lost grant, one in flush()
        assert!(*queries.lock().unwrap() >= 2);
        assert!(start.elapsed() >= Duration::from_millis(50));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn lost_write_is_sent_again_go_back_n() {
        let (device, m) = start(
            "credit/lost_write",
            Config::new(3200, 800).drain_at(0, Duration::ZERO),
        );
        let dropped = Arc::new(Mutex::new(false));
        let d = dropped.clone();
        device.filter_requests(move |request| {
            let mut d = d.lock().unwrap();
            // the 4th item
            if !*d && write_pos(request) == Some(300) {
                *d = true;
                return true;
            }
            false
        });
        let client = connect(&device).await;
        let mut plan = client.plan().await.unwrap();
        plan.set_credit_config(CreditConfig {
            query_interval: Duration::from_millis(50),
            stall_timeout: Some(Duration::from_secs(2)),
        });
        let bytes = data(1000);
        for chunk in bytes.chunks(100) {
            plan.send_bytes(chunk).await.unwrap();
        }
        plan.flush().await.unwrap();
        assert!(*dropped.lock().unwrap());
        assert_eq!(m.lock().unwrap().plan, bytes);
        assert_eq!(m.lock().unwrap().overruns, 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn lost_last_write_is_found_by_flush() {
        let (device, m) = start(
            "credit/lost_last",
            Config::new(3200, 800).drain_at(0, Duration::ZERO),
        );
        let dropped = Arc::new(Mutex::new(false));
        let d = dropped.clone();
        device.filter_requests(move |request| {
            let mut d = d.lock().unwrap();
            if !*d && write_pos(request) == Some(900) {
                *d = true;
                return true;
            }
            false
        });
        let client = connect(&device).await;
        let mut plan = client.plan().await.unwrap();
        plan.set_credit_config(CreditConfig {
            query_interval: Duration::from_millis(50),
            stall_timeout: Some(Duration::from_secs(2)),
        });
        let bytes = data(1000);
        for chunk in bytes.chunks(100) {
            plan.send_bytes(chunk).await.unwrap();
        }
        // nothing after the lost write shows the gap, the query in flush() does
        plan.flush().await.unwrap();
        assert!(*dropped.lock().unwrap());
        assert_eq!(m.lock().unwrap().plan, bytes);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn lost_open_is_sent_again() {
        let (device, m) = start(
            "credit/lost_open",
            Config::new(16, 8).drain_at(0, Duration::ZERO),
        );
        let dropped = Arc::new(Mutex::new(false));
        let d = dropped.clone();
        device.filter_requests(move |request| {
            let mut d = d.lock().unwrap();
            if !*d && is_open(request) {
                *d = true;
                return true;
            }
            false
        });
        let client = connect(&device).await;
        let mut plan = client.plan().await.unwrap();
        plan.set_credit_config(CreditConfig {
            query_interval: Duration::from_millis(30),
            stall_timeout: Some(Duration::from_secs(2)),
        });
        // opens by itself
        plan.send_bytes(&[1, 2, 3]).await.unwrap();
        plan.flush().await.unwrap();
        assert!(*dropped.lock().unwrap());
        assert_eq!(m.lock().unwrap().plan, [1, 2, 3]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reopen_starts_a_new_session() {
        let (device, m) = start(
            "credit/reopen",
            Config::new(256, 128).drain_at(0, Duration::ZERO),
        );
        let positions = Arc::new(Mutex::new(vec![]));
        let p = positions.clone();
        device.filter_requests(move |request| {
            if let Some(pos) = write_pos(request) {
                p.lock().unwrap().push(pos);
            }
            false
        });
        let client = connect(&device).await;
        let mut plan = client.plan().await.unwrap();
        plan.open().await.unwrap();
        plan.send_bytes(&[1; 100]).await.unwrap();
        plan.send_bytes(&[2; 100]).await.unwrap();
        plan.close().await.unwrap();
        plan.open().await.unwrap();
        plan.send_bytes(&[3; 50]).await.unwrap();
        plan.close().await.unwrap();
        eventually(&m, |m| m.closed == 2).await;
        assert_eq!(*positions.lock().unwrap(), [0, 100, 0]);
        assert_eq!(m.lock().unwrap().plan.len(), 250);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn mixed_sizes_and_empty_items() {
        let (device, m) = start(
            "credit/mixed",
            // the largest item fills the buffer: a re-grant after any byte freed, see SinkCredit::with_threshold
            Config::new(1000, 1).drain_at(0, Duration::ZERO),
        );
        let client = connect(&device).await;
        let mut plan = client.plan().await.unwrap();
        let start = Instant::now();
        for len in [600, 1, 1000, 0, 7, 0] {
            plan.send_bytes(&vec![0xAB; len]).await.unwrap();
        }
        plan.flush().await.unwrap();
        // never waited for a CreditQuery (1 s)
        assert!(start.elapsed() < Duration::from_millis(500));
        // an empty item takes one position
        assert_eq!(plan.credit_progress(), Some((1610, 1610)));
        assert_eq!(m.lock().unwrap().plan.len(), 1608);
        assert_eq!(m.lock().unwrap().overruns, 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn item_bigger_than_buffer_fails_right_away() {
        let (device, m) = start(
            "credit/too_big",
            Config::new(64, 32).drain_at(0, Duration::ZERO),
        );
        let client = connect(&device).await;
        let mut plan = client.plan().await.unwrap();
        plan.send_bytes(&[1; 64]).await.unwrap();
        let start = Instant::now();
        let r = plan.send_bytes(&[2; 65]).await;
        assert!(
            matches!(
                r,
                Err(StreamError::ItemTooBig {
                    len: 65,
                    buffer: 64
                })
            ),
            "{r:?}"
        );
        assert!(start.elapsed() < Duration::from_millis(500), "no waiting");
        // the session goes on
        plan.send_bytes(&[3; 10]).await.unwrap();
        plan.flush().await.unwrap();
        assert_eq!(m.lock().unwrap().plan.len(), 74);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stuck_device_times_out() {
        let (device, _m) = start(
            "credit/stuck",
            Config::new(64, 64).drain_at(1_000_000, Duration::ZERO),
        );
        let client = connect(&device).await;
        let mut plan = client.plan().await.unwrap();
        plan.set_credit_config(CreditConfig {
            query_interval: Duration::from_millis(20),
            stall_timeout: Some(Duration::from_millis(150)),
        });
        plan.send_bytes(&[0; 64]).await.unwrap();
        // never freed
        let start = Instant::now();
        let r = plan.send_bytes(&[0; 1]).await;
        assert!(
            matches!(r, Err(StreamError::Other(Error::Timeout))),
            "{r:?}"
        );
        assert!(start.elapsed() >= Duration::from_millis(100));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn typed_values_and_try_send() {
        let (device, m) = start("credit/typed", Config::new(4, 2));
        let client = connect(&device).await;
        let mut points = client.points().await.unwrap();
        // credit for 4 points
        let expected: Vec<Point> = (0..50).map(|i| Point { x: i, y: -i }).collect();
        let mut would_block = 0;
        for p in &expected {
            while !points.try_send(p).unwrap() {
                would_block += 1;
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        }
        assert!(would_block > 0, "credit for 4 points runs out");
        while !points.try_flush().unwrap() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert_eq!(m.lock().unwrap().points, expected);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn units_named_by_the_host() {
        let (device, m) = start("credit/units", Config::new(4, 2));
        let client = connect(&device).await;
        let mut points = client.points().await.unwrap();
        points.send(Point { x: 1, y: 1 }).await.unwrap();
        points.send_units(Point { x: 2, y: 2 }, 5).await.unwrap();
        points.send_units(Point { x: 3, y: 3 }, 1000).await.unwrap();
        points.flush().await.unwrap();
        assert_eq!(m.lock().unwrap().point_units, [1, 5, 1000]);
        assert_eq!(points.credit_progress(), Some((1006, 1006)));
        let r = points.send_units(Point { x: 4, y: 4 }, 0).await;
        assert!(matches!(r, Err(StreamError::BadUnits { units: 0 })), "{r:?}");

        // byte sinks count bytes, an empty item takes one position
        let mut plan = client.plan().await.unwrap();
        plan.send_bytes(&[1; 3]).await.unwrap();
        plan.send_bytes(&[]).await.unwrap();
        plan.flush().await.unwrap();
        assert_eq!(plan.credit_progress(), Some((4, 4)));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn status_read_shows_the_session() {
        let (device, _m) = start(
            "credit/status",
            Config::new(256, 128).drain_at(1000, Duration::ZERO),
        );
        let client = connect(&device).await;
        let status = client.plan_status().read().await.unwrap();
        assert!(!status.open);
        assert_eq!(status.session, None);
        let mut plan = client.plan().await.unwrap();
        plan.send_bytes(&[7; 100]).await.unwrap();
        plan.flush().await.unwrap();
        let status = client.plan_status().read().await.unwrap();
        assert!(status.open);
        assert!(status.session.is_some_and(|s| s.0 != 0));
        assert_eq!(status.next.0, 100);
        let store = status.store.unwrap();
        // nothing drained yet
        assert_eq!(store.used_bytes.0, 100);
        assert_eq!(store.capacity_bytes.0, 256);
        plan.close().await.unwrap();
        let status = client.plan_status().read().await.unwrap();
        assert!(!status.open);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn device_that_lost_the_session_is_reported() {
        let (device, m) = start(
            "credit/reboot",
            Config::new(256, 128).drain_at(0, Duration::ZERO),
        );
        let client = connect(&device).await;
        let mut plan = client.plan().await.unwrap();
        plan.set_credit_config(CreditConfig {
            query_interval: Duration::from_millis(30),
            stall_timeout: Some(Duration::from_secs(2)),
        });
        plan.send_bytes(&[1; 50]).await.unwrap();
        plan.flush().await.unwrap();
        m.lock().unwrap().reboot = true;
        // taken by the device as its last write before the "reboot", then lost with it
        plan.send_bytes(&[2; 50]).await.unwrap();
        // the device answers the next write with Close, the host opens its session again and hears it is gone
        let r = async {
            plan.send_bytes(&[3; 50]).await?;
            plan.flush().await
        }
        .await;
        assert!(
            matches!(
                r,
                Err(StreamError::SinkSessionLost {
                    device_at: 0,
                    acked: 50
                })
            ),
            "{r:?}"
        );
        // a new session after that
        plan.send_bytes(&[4; 10]).await.unwrap();
        plan.flush().await.unwrap();
        assert_eq!(plan.credit_progress(), Some((10, 10)));
        let consumed = m.lock().unwrap().plan.clone();
        assert_eq!(consumed, [[1; 50].as_slice(), &[4; 10]].concat());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn array_of_credit_sinks() {
        let (device, m) = start("credit/array", Config::new(4, 2));
        let client = connect(&device).await;
        let mut lane1 = client.lanes(1).await.unwrap();
        let mut lane2 = client.lanes(2).await.unwrap();
        for i in 0..20u8 {
            lane1.send_bytes(&[i; 10]).await.unwrap();
            lane2.send_bytes(&[i + 100; 7]).await.unwrap();
        }
        lane1.flush().await.unwrap();
        lane2.flush().await.unwrap();
        let m = m.lock().unwrap();
        assert_eq!(m.lanes[1].len(), 200);
        assert_eq!(m.lanes[2].len(), 140);
        assert_eq!(m.lanes[2][139], 119);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn plain_sink_writes_have_no_position() {
        let (device, m) = start("credit/plain", Config::new(4, 2));
        let raw = Arc::new(Mutex::new(vec![]));
        let r = raw.clone();
        device.filter_requests(move |request| {
            let mut rd = BufReader::new(request);
            if let Ok(Request {
                kind: RequestKind::Write { data },
                ..
            }) = Request::des_shrink_wrap(&mut rd)
            {
                r.lock().unwrap().push(data.0.to_vec());
            }
            false
        });
        let client = connect(&device).await;
        let mut plain = client.plain().await.unwrap();
        assert!(!plain.is_credit());
        plain.send_bytes(&[5, 6, 7]).await.unwrap();
        client.finish().call().await.unwrap();
        assert_eq!(*raw.lock().unwrap(), [vec![5, 6, 7]]);
        assert_eq!(m.lock().unwrap().plain, [vec![5, 6, 7]]);
    }

    #[test]
    fn blocking_client() {
        let (device, m) = start(
            "credit/blocking",
            Config::new(400, 200).drain_at(300, Duration::from_millis(5)),
        );
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let client = Client::config(|c| c.in_process_path(device.path()))
            .connect_blocking()
            .expect("connect");
        let mut plan = client.plan_blocking().unwrap();
        let bytes = data(4000);
        for chunk in bytes.chunks(50) {
            plan.send_bytes_blocking(chunk).unwrap();
        }
        plan.flush_blocking().unwrap();
        plan.close_blocking().unwrap();
        let mut points = client.points_blocking().unwrap();
        for i in 0..10 {
            points.send_blocking(Point { x: i, y: i }).unwrap();
        }
        points.flush_blocking().unwrap();
        for _ in 0..1000 {
            if m.lock().unwrap().closed == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let m = m.lock().unwrap();
        assert_eq!(m.plan, bytes);
        assert_eq!(m.points.len(), 10);
        assert_eq!(m.overruns, 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn async_server() {
        let m = Arc::new(Mutex::new(Model::default()));
        let server = async_server::Server {
            m: m.clone(),
            credit: SinkCredit::new(256).with_threshold(128),
            other: SinkCredit::new(64),
        };
        let device = tests_common::start_device_async(
            "credit/async_server",
            server,
            credit_sink_api::CREDIT_SINKS_FULL_GID,
            async_server::api_impl::api_hash(),
        );
        let client = connect(&device).await;
        let mut plan = client.plan().await.unwrap();
        let bytes = data(2000);
        for chunk in bytes.chunks(60) {
            plan.send_bytes(chunk).await.unwrap();
        }
        plan.flush().await.unwrap();
        assert_eq!(m.lock().unwrap().plan, bytes);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dynamic_client() {
        use wire_weaver_client::ww_numeric::NumericValue;
        use wire_weaver_client::ww_self::FieldsValueOwned;
        use wire_weaver_client::ww_self::ValueOwned;
        use wire_weaver_client::{ClientConfig, DynClient, DynResource, DynResourceKind};

        let (device, m) = start("credit/dynamic", Config::new(4, 2));
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../credit_sink_api");
        let bundle = wire_weaver_core::load(&path, Some("CreditSinks".into()), false).unwrap();
        let client = DynClient::from_config(ClientConfig::new().in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");
        let root = DynResource::root(Arc::new(bundle));
        let points = root.child("points").unwrap();
        assert!(matches!(
            points.kind().unwrap(),
            DynResourceKind::Stream { flow: Some(_), .. }
        ));
        let mut sink = points.sink(client.cmd()).await.unwrap();
        assert!(sink.is_credit());
        let point = |x: i16| ValueOwned::Struct {
            fields: FieldsValueOwned::Named(vec![
                ("x".into(), ValueOwned::Numeric(NumericValue::I16(x))),
                ("y".into(), ValueOwned::Numeric(NumericValue::I16(-x))),
            ]),
        };
        for x in 0..20 {
            sink.send(&point(x)).await.unwrap();
        }
        sink.flush().await.unwrap();
        let got = m.lock().unwrap().points.clone();
        assert_eq!(got.len(), 20);
        assert_eq!(got[19], Point { x: 19, y: -19 });
    }

    #[test]
    fn scaffold_has_credit_handlers() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../credit_sink_api");
        let bundle = wire_weaver_core::load(&path, Some("CreditSinks".into()), false).unwrap();
        let config = wire_weaver_core::ServerScaffoldConfig {
            server_struct: "Server".into(),
            no_alloc: true,
            use_async: false,
            handler_model: String::new(),
            method_model: String::new(),
            property_model: String::new(),
            const_model: String::new(),
            medium: None,
            omit: vec![],
            observe_cells: vec![],
        };
        let code = wire_weaver_core::gen_server_scaffold(&bundle, &config).unwrap();
        let code: String = code.split_whitespace().collect::<Vec<_>>().join(" ");
        for expected in [
            "pub plan_credit: ww_stream::sink::SinkCredit,",
            "fn credit_plan(&mut self, ) -> &mut ww_stream::sink::SinkCredit { &mut self.plan_credit }",
            "fn credit_lanes(&mut self, _index_chain: [UNib32; 1], ) -> &mut ww_stream::sink::SinkCredit",
            "fn write_plan(",
        ] {
            assert!(code.contains(expected), "{expected}\n{code}");
        }
        assert!(!code.contains("plain_credit"));
    }

    #[test]
    fn max_item_bytes_checks_the_buffer() {
        use sync_server::api_impl::stream_data_ser;
        // Point is `sized`, an evolvable struct would be None
        assert_eq!(Ser::POINTS_MAX_ITEM_BYTES, Some(4));
        assert_eq!(stream_data_ser().points_max_item_bytes(), Some(4));
        // checked at compile time in a const
        const CREDIT: SinkCredit = SinkCredit::sized_for(64, Ser::POINTS_MAX_ITEM_BYTES);
        assert_eq!(CREDIT.capacity(), 64);
        assert!(SinkCredit::try_sized_for(3, Ser::POINTS_MAX_ITEM_BYTES).is_err());
        // byte slices without a bound have none, with MaxLen the bound is the byte count (sent as is)
        assert_eq!(Ser::PLAN_MAX_ITEM_BYTES, None);
        assert_eq!(Ser::BLOCKS_MAX_ITEM_BYTES, Some(4));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bounded_bytes_too_long_is_never_accepted() {
        let (device, m) = start("credit/bounded", Config::new(4, 2));
        let client = connect(&device).await;
        let mut blocks = client.blocks().await.unwrap();
        blocks.set_credit_config(CreditConfig {
            query_interval: Duration::from_millis(20),
            stall_timeout: Some(Duration::from_millis(200)),
        });
        blocks.send_bytes(&[1, 2, 3, 4]).await.unwrap();
        blocks.send_bytes(&[]).await.unwrap();
        blocks.flush().await.unwrap();
        assert_eq!(m.lock().unwrap().blocks, [vec![1, 2, 3, 4], vec![]]);
        // refused before it takes a position: the device keeps asking for it, the host gives up
        blocks.send_bytes(&[1, 2, 3, 4, 5]).await.unwrap();
        let r = blocks.flush().await;
        assert!(
            matches!(r, Err(StreamError::Other(Error::Timeout))),
            "{r:?}"
        );
        assert_eq!(m.lock().unwrap().blocks.len(), 2);
        // 4 bytes, an empty item (one position), and the refused 5 bytes
        assert_eq!(blocks.credit_progress(), Some((5, 10)));
    }
}

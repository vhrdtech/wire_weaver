//! Device tunnels through the ww hub (SRV-29): several clients of one in-process device, each through its own tunnel,
//! with the hub's event loop to the device in between.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver_client::internal::Command;
    use wire_weaver_client::{ClientConfig, Commander, WwClient};
    use ww_client_server::{PathKindOwned, RequestKindOwned, RequestOwned};
    use ww_hub::{Bound, Exit, HubConfig, Shutdown};

    /// What the device saw: stream opens and closes, introspection requests
    #[derive(Default)]
    struct Seen {
        log: Vec<String>,
        introspects: usize,
        /// `bus[i].speed`
        speed: [u32; 2],
    }

    mod server {
        use super::*;
        use tests_common::TestProcessEvents;

        pub struct Server {
            pub seen: Arc<Mutex<Seen>>,
        }

        impl Server {
            fn add(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                a: u32,
                b: u32,
            ) -> RpcResult<u32> {
                Ready(a.wrapping_add(b))
            }

            fn ticks(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
                self.seen.lock().unwrap().log.push("open ticks".into());
                Ready(())
            }

            fn close_ticks(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {
                self.seen.lock().unwrap().log.push("close ticks".into());
            }

            fn sideband_ticks(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }

            fn other(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
                self.seen.lock().unwrap().log.push("open other".into());
                Ready(())
            }

            fn close_other(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {
                self.seen.lock().unwrap().log.push("close other".into());
            }

            fn sideband_other(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }

            fn valid_indices_root_bus(&mut self) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..2)
            }

            // ---- hub_api::Bus at bus[] ----

            /// Sends a frame: `Common(InUse)` from the hub while another program holds the bus.
            fn bus_send(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _index_chain: [UNib32; 1],
                id: u32,
            ) -> RpcResult<Result<u32, hub_api::BusError<'_>>> {
                Ready(Ok(id))
            }

            /// Bit rate.
            fn set_bus_speed(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
                value: u32,
            ) -> SetResult<hub_api::BusError<'_>> {
                self.seen.lock().unwrap().speed[index_chain[0].0 as usize] = value;
                SetResult::Set
            }

            /// Bit rate.
            fn get_bus_speed(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
            ) -> GetResult<u32, hub_api::BusError<'_>> {
                GetResult::Value(self.seen.lock().unwrap().speed[index_chain[0].0 as usize])
            }

            /// Received frames.
            ///
            /// Opens the stream (a new session): `Ok` / `Ready` starts it.
            fn bus_rx(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _index_chain: [UNib32; 1],
            ) -> RpcResult<()> {
                RpcResult::Ready(())
            }

            /// The host closed the stream.
            fn close_bus_rx(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _index_chain: [UNib32; 1],
            ) {
            }

            fn sideband_bus_rx(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _index_chain: [UNib32; 1],
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                // TODO: None sends no reply
                None
            }

            // ---- hub_api::Port at port ----

            fn port_ping(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<u8> {
                Ready(1)
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                hub_api :: HubTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                introspect = "no_docs",
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

    pub struct Client {
        pub cmd: Commander,
    }

    impl WwClient for Client {
        fn default_config() -> ClientConfig {
            ClientConfig::default()
        }

        fn from_cmd(cmd: Commander) -> Self {
            Client { cmd }
        }
    }

    mod api_client {
        wire_weaver::ww_codegen!(hub_api :: HubTest for super::Client, client = "std_client");
    }

    use server::api_impl::stream_data_ser;

    struct Bench {
        device: TestDevice,
        seen: Arc<Mutex<Seen>>,
        dir: PathBuf,
        key: String,
        hub: tokio::task::JoinHandle<Exit>,
        shutdown: Shutdown,
    }

    impl Bench {
        fn socket(&self) -> String {
            self.dir
                .join(ww_local_socket::files::HUB_SOCKET)
                .display()
                .to_string()
        }

        async fn client(&self) -> Client {
            self.client_with(|c| c).await
        }

        async fn client_with(&self, f: impl Fn(ClientConfig) -> ClientConfig) -> Client {
            let (socket, key) = (self.socket(), self.key.clone());
            Client::config(|c| f(c.hub_device_at(socket.clone(), key.clone())))
                .connect()
                .await
                .expect("connect through the hub")
        }

        async fn stop(self) {
            self.shutdown.shutdown();
            assert_eq!(self.hub.await.unwrap(), Exit::Shutdown);
            drop(self.device);
        }
    }

    fn runtime_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ww-hub-e2e-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn start(name: &str, linger: Duration) -> Bench {
        let _ = tracing_subscriber::fmt::try_init();
        let seen = Arc::new(Mutex::new(Seen::default()));
        let path = format!("hub/{name}");
        let device = tests_common::start_device(
            &path,
            server::Server { seen: seen.clone() },
            hub_api::HUB_TEST_FULL_GID,
            server::api_impl::api_hash(),
        );
        let counted = seen.clone();
        device.filter_requests(move |bytes| {
            if RequestOwned::from_ww_bytes_owned(bytes)
                .is_ok_and(|r| matches!(r.kind, RequestKindOwned::Introspect))
            {
                counted.lock().unwrap().introspects += 1;
            }
            false
        });
        let dir = runtime_dir(name);
        let bound = Bound::bind(HubConfig {
            runtime_dir: dir.clone(),
            idle_exit: Duration::from_secs(60),
            linger,
            version: ww_hub::HUB_VERSION.into(),
        })
        .unwrap()
        .expect("no other hub in this dir");
        let shutdown = bound.shutdown_handle();
        Bench {
            device,
            seen,
            dir,
            key: format!("inproc:{path}"),
            hub: tokio::spawn(bound.run()),
            shutdown,
        }
    }

    async fn eventually(what: &str, f: impl Fn() -> bool) {
        for _ in 0..1000 {
            if f() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        panic!("{what}: not within 1 s");
    }

    fn log(seen: &Arc<Mutex<Seen>>) -> Vec<String> {
        seen.lock().unwrap().log.clone()
    }

    /// 100 calls in flight, then their answers.
    async fn calls(c: &mut Client, base: u32) {
        let mut pending = vec![];
        for i in 0..100 {
            pending.push(c.add(base, i).send().await.unwrap());
        }
        for (i, p) in pending.into_iter().enumerate() {
            assert_eq!(p.reply().await.unwrap(), base + i as u32);
        }
    }

    /// Two clients call the device at the same time: both start their seqs at 1, the hub gives the device its own
    /// and every answer reaches its caller.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn two_clients_calls_remapped() {
        let bench = start("calls", Duration::from_secs(30));
        let mut a = bench.client().await;
        let mut b = bench.client().await;
        tokio::join!(calls(&mut a, 1000), calls(&mut b, 2000));
        bench.stop().await;
    }

    /// A stream opened by two clients is opened on the device once, its data goes to both, and it is closed on the
    /// device when the last one closes it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn stream_shared_opened_once_closed_by_last() {
        let bench = start("streams", Duration::from_secs(30));
        let a = bench.client().await;
        let b = bench.client().await;
        let mut sa = a.ticks().call().await.unwrap();
        let mut sb = b.ticks().call().await.unwrap();
        assert_eq!(log(&bench.seen), ["open ticks"]);

        let mut scratch = [0u8; 64];
        for v in [1u32, 2, 3] {
            bench
                .device
                .send(stream_data_ser().ticks(&v, &mut scratch).unwrap());
        }
        for v in [1u32, 2, 3] {
            assert_eq!(sa.recv().await.unwrap(), v);
            assert_eq!(sb.recv().await.unwrap(), v);
        }
        // only who opened it gets it: b's other stream isn't a's
        let mut other = b.other().call().await.unwrap();
        bench
            .device
            .send(stream_data_ser().other(&7, &mut scratch).unwrap());
        assert_eq!(other.recv().await.unwrap(), 7);

        sa.close().await.unwrap();
        bench
            .device
            .send(stream_data_ser().ticks(&4, &mut scratch).unwrap());
        assert_eq!(sb.recv().await.unwrap(), 4);
        assert!(!log(&bench.seen).contains(&"close ticks".to_string()));
        sb.close().await.unwrap();
        let seen = bench.seen.clone();
        eventually("closed on the device", || {
            log(&seen).contains(&"close ticks".to_string())
        })
        .await;
        bench.stop().await;
    }

    /// A client killed in the middle of a stream (its process gone, no `Disconnect`): the other one keeps
    /// receiving, the device's stream stays open for it and is closed when it closes.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn killed_client_mid_stream() {
        let bench = start("killed", Duration::from_secs(30));
        let a = bench.client().await;
        let mut sa = a.ticks().call().await.unwrap();

        // b runs on a runtime of its own, dropped with everything on it, as a killed process would be
        let (socket, key) = (bench.socket(), bench.key.clone());
        let (got_tx, got_rx) = std::sync::mpsc::channel();
        let (kill_tx, kill_rx) = std::sync::mpsc::channel::<()>();
        let b = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                let b = Client::config(|c| c.hub_device_at(socket.clone(), key.clone()))
                    .connect()
                    .await
                    .unwrap();
                let mut sb = b.ticks().call().await.unwrap();
                got_tx.send(None).unwrap();
                got_tx.send(Some(sb.recv().await.unwrap())).unwrap();
                // never disconnects nor closes
                std::mem::forget(sb);
                std::mem::forget(b);
            });
            kill_rx.recv().unwrap();
            rt.shutdown_background();
        });
        assert_eq!(got_rx.recv().unwrap(), None);
        let mut scratch = [0u8; 64];
        bench
            .device
            .send(stream_data_ser().ticks(&1, &mut scratch).unwrap());
        assert_eq!(sa.recv().await.unwrap(), 1);
        assert_eq!(got_rx.recv().unwrap(), Some(1));

        kill_tx.send(()).unwrap();
        b.join().unwrap();
        for v in 2..20u32 {
            bench
                .device
                .send(stream_data_ser().ticks(&v, &mut scratch).unwrap());
            assert_eq!(sa.recv().await.unwrap(), v);
        }
        assert_eq!(log(&bench.seen), ["open ticks"], "still open for a");
        sa.close().await.unwrap();
        let seen = bench.seen.clone();
        eventually("closed on the device", || {
            log(&seen) == ["open ticks", "close ticks"]
        })
        .await;
        bench.stop().await;
    }

    /// Introspection is fetched from the device once, later clients get it from the hub.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn introspection_from_the_cache() {
        let bench = start("introspect", Duration::from_secs(30));
        let a = bench.client().await;
        let b = bench.client().await;
        let first = a.cmd.introspect().download().await.unwrap().unwrap();
        let second = b.cmd.introspect().download().await.unwrap().unwrap();
        assert_eq!(
            first.to_ww_bytes_owned().unwrap(),
            second.to_ww_bytes_owned().unwrap()
        );
        assert_eq!(bench.seen.lock().unwrap().introspects, 1);
        bench.stop().await;
    }

    /// The hub reads only seq, path and kind: a client without client meta (built before CLT-41) and a request of a
    /// newer envelope, which the hub can't decode, go through as they are, the answer with the client's seq.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn other_client_versions_pass_through() {
        let bench = start("versions", Duration::from_secs(30));
        let mut old = bench.client_with(|c| c.no_client_meta()).await;
        assert_eq!(old.add(2, 3).call().await.unwrap(), 5);

        let new = bench.client().await;
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        new.cmd
            .send(Command::SendMessage {
                bytes: unknown_kind_request(),
                done_tx: Some((done_tx, Duration::from_secs(1))),
            })
            .await
            .unwrap();
        // the device can't read it either and says so, under the client's seq
        match done_rx.await.unwrap() {
            Err(wire_weaver_client::Error::RemoteError(_)) => {}
            other => panic!("expected the device's error, got {other:?}"),
        }
        bench.stop().await;
    }

    /// A request whose kind is a discriminant no `ww_client_server` has yet.
    fn unknown_kind_request() -> Vec<u8> {
        let request = |kind| {
            RequestOwned {
                seq: wire_weaver::shrink_wrap::UVlq32Backfill(0),
                path_kind: PathKindOwned::Absolute { path: vec![] },
                kind,
            }
            .to_ww_bytes_owned()
            .unwrap()
        };
        let (read, introspect) = (
            request(RequestKindOwned::Read),
            request(RequestKindOwned::Introspect),
        );
        assert_eq!(read.len(), introspect.len());
        let mut bytes = read.clone();
        let at = (0..read.len())
            .find(|i| read[*i] != introspect[*i])
            .unwrap();
        // the kind is a nibble: set it to 15
        let diff = read[at] ^ introspect[at];
        bytes[at] |= if diff & 0xF0 != 0 { 0xF0 } else { 0x0F };
        assert!(RequestOwned::from_ww_bytes_owned(&bytes).is_err());
        bytes
    }

    /// A device is opened by the first tunnel and closed `linger` after the last one left; the next client opens
    /// it again.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn device_closed_after_linger_and_reopened() {
        let bench = start("linger", Duration::from_millis(200));
        let hub = ww_hub::client::HubClient::config(|c| c.hub_api_at(bench.socket()))
            .connect()
            .await
            .unwrap();
        let open = || async {
            hub.registry()
                .devices()
                .call()
                .await
                .unwrap()
                .into_iter()
                .find(|d| d.key == bench.key)
                .map(|d| (d.state, d.tunnels))
        };
        assert_eq!(open().await, None);
        let mut a = bench.client().await;
        assert_eq!(a.add(1, 1).call().await.unwrap(), 2);
        assert_eq!(open().await, Some((ww_hub_api::DeviceStateOwned::Open, 1)));
        a.cmd.disconnect().asynch().await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            open().await.map(|(s, _)| s),
            Some(ww_hub_api::DeviceStateOwned::Open),
            "lingers"
        );
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(open().await, None, "closed after linger");
        let mut b = bench.client().await;
        assert_eq!(b.add(2, 2).call().await.unwrap(), 4);
        drop(hub);
        bench.stop().await;
    }

    // ---- claims (SRV-34) ----

    use hub_api::BusErrorOwned;
    use wire_weaver_client::{CallError, ClientMeta};
    use ww_hub::client::HubClient;
    use ww_hub_api::{ClaimErrorOwned, ClaimKind, EventOwned};

    /// `bus[0]`: resource 3 of the root, index 0
    fn bus0() -> Vec<UNib32> {
        vec![UNib32(3), UNib32(0)]
    }

    /// One program: its hub API client and its device client, one client instance (a shared [ClientMeta]).
    struct Program {
        hub: HubClient,
        dev: Client,
    }

    impl Bench {
        async fn program(&self, name: &str) -> Program {
            let meta = ClientMeta::new().program(name);
            let socket = self.socket();
            let hub = HubClient::config(|c| c.hub_api_at(socket.clone()).client_meta(meta.clone()))
                .connect()
                .await
                .unwrap();
            let dev = self.client_with(|c| c.client_meta(meta.clone())).await;
            Program { hub, dev }
        }

        /// Claims `path` for `p`, the claim's id.
        async fn claim(&self, p: &Program, path: Vec<UNib32>, ttl_s: Option<u32>) -> u32 {
            p.hub
                .claims()
                .claim(self.key.clone(), path, "flashing".into(), ttl_s)
                .call()
                .await
                .expect("claimed")
        }

        /// Why `p` can't claim `path`.
        async fn claim_refused(&self, p: &Program, path: Vec<UNib32>) -> ClaimErrorOwned {
            let r = p
                .hub
                .claims()
                .claim(self.key.clone(), path, "flashing".into(), None)
                .call()
                .await;
            device_error(r.expect_err("refused"))
        }
    }

    /// How the hub names a holder: program and pid.
    fn by(program: &str) -> String {
        format!("{program} pid {}", std::process::id())
    }

    /// The device's (here: the hub's) error, panics on any other.
    fn device_error<E: std::fmt::Debug>(e: CallError<E>) -> E {
        match e {
            CallError::Device(e) => e,
            other => panic!("expected an error of the API, got {other:?}"),
        }
    }

    /// What B's call or write at A's claim gets.
    fn refused<T: std::fmt::Debug>(r: Result<T, CallError<BusErrorOwned>>) -> BusErrorOwned {
        device_error(r.expect_err("refused"))
    }

    /// `Common(InUse)` naming `program` of this test process.
    fn in_use(program: &str) -> BusErrorOwned {
        BusErrorOwned::Common(CommonErrorOwned::InUse {
            by: MaxLen::new(by(program)).unwrap(),
        })
    }

    /// A holds `bus[0]`: B's calls and writes there get `Common(InUse)` naming A, from the hub; its reads, streams
    /// and calls elsewhere go on; A's own calls go through.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn claimed_bus_refuses_others_calls_and_writes() {
        let bench = start("claim", Duration::from_secs(30));
        let a = bench.program("alice").await;
        let b = bench.program("bob").await;
        let mut rx = b.dev.bus(0).rx().call().await.unwrap();

        let id = bench.claim(&a, bus0(), None).await;
        let claims = a
            .hub
            .claims()
            .claims(bench.key.clone())
            .call()
            .await
            .unwrap();
        assert_eq!(claims.len(), 1);
        let entry = &claims[0];
        assert_eq!(
            (entry.id, &entry.path, entry.kind, entry.label.as_str()),
            (id, &bus0(), ClaimKind::Exclusive, "CAN1, front")
        );
        assert_eq!(
            (entry.purpose.as_str(), entry.holder.program.as_str()),
            ("flashing", "alice")
        );

        assert_eq!(refused(b.dev.bus(0).send(7).call().await), in_use("alice"));
        assert_eq!(
            refused(b.dev.bus(0).write_speed(250_000).write().await),
            in_use("alice")
        );
        assert_eq!(
            bench.seen.lock().unwrap().speed,
            [0, 0],
            "never reached the device"
        );

        // reads, streams and the other bus stay open
        a.dev.bus(0).write_speed(500_000).write().await.unwrap();
        assert_eq!(b.dev.bus(0).read_speed().read().await.unwrap(), 500_000);
        let mut scratch = [0u8; 64];
        bench
            .device
            .send(stream_data_ser().bus(0).rx(&9, &mut scratch).unwrap());
        assert_eq!(rx.recv().await.unwrap(), 9);
        assert_eq!(b.dev.bus(1).send(8).call().await.unwrap(), 8);
        assert_eq!(a.dev.bus(0).send(7).call().await.unwrap(), 7);

        // released: B goes through
        assert_eq!(
            b.hub
                .claims()
                .release(id)
                .call()
                .await
                .map_err(device_error),
            Err(ww_hub_api::ReleaseError::NotHolder)
        );
        a.hub.claims().release(id).call().await.unwrap();
        assert_eq!(b.dev.bus(0).send(7).call().await.unwrap(), 7);
        bench.stop().await;
    }

    /// What can't be claimed, and overlapping claims of two programs: the same resource, one above it (the root),
    /// one sharing its pins (`port` on PA11); one program nests its own.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn claim_errors_and_overlap() {
        let bench = start("claim-errors", Duration::from_secs(30));
        let a = bench.program("alice").await;
        let b = bench.program("bob").await;
        assert_eq!(
            bench.claim_refused(&a, vec![UNib32(0)]).await,
            ClaimErrorOwned::NotClaimable,
            "add"
        );
        assert_eq!(
            bench.claim_refused(&a, vec![UNib32(9)]).await,
            ClaimErrorOwned::BadPath
        );
        let unknown = a
            .hub
            .claims()
            .claim("inproc:nope".into(), vec![], String::new(), None)
            .call()
            .await;
        assert_eq!(
            device_error(unknown.expect_err("refused")),
            ClaimErrorOwned::UnknownDevice
        );

        let id = bench.claim(&a, bus0(), None).await;
        // nested and overlapping claims of one program
        bench.claim(&a, vec![UNib32(4)], None).await;
        let root = bench.claim(&a, vec![], None).await;
        a.hub.claims().release(root).call().await.unwrap();

        match bench.claim_refused(&b, bus0()).await {
            ClaimErrorOwned::InUse { claim } => assert_eq!(claim.id, id),
            other => panic!("expected InUse, got {other:?}"),
        }
        for path in [vec![], vec![UNib32(4)]] {
            match bench.claim_refused(&b, path.clone()).await {
                ClaimErrorOwned::Overlap { claim } => {
                    assert_eq!(claim.holder.program, "alice", "{path:?}")
                }
                other => panic!("{path:?}: expected Overlap, got {other:?}"),
            }
        }
        // the other bus shares nothing with bus[0]
        bench.claim(&b, vec![UNib32(3), UNib32(1)], None).await;
        bench.stop().await;
    }

    /// A port claimed for observing: B's calls are refused with an error event naming A (`ping` has no error type of
    /// its own to put `Common(InUse)` in), A's go through.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn observe_claim_refuses_untyped_calls() {
        let bench = start("claim-observe", Duration::from_secs(30));
        let a = bench.program("alice").await;
        let b = bench.program("bob").await;
        let id = bench.claim(&a, vec![UNib32(4)], None).await;
        let claims = b
            .hub
            .claims()
            .claims(bench.key.clone())
            .call()
            .await
            .unwrap();
        assert_eq!((claims[0].id, claims[0].kind), (id, ClaimKind::Observe));
        match b.dev.port().ping().call().await {
            Err(wire_weaver_client::Error::RemoteError(e)) => {
                assert!(
                    matches!(&e.kind, ww_client_server::ErrorKindOwned::UserStr(s) if *s == format!("in use by {}", by("alice"))),
                    "{e:?}"
                )
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert_eq!(a.dev.port().ping().call().await.unwrap(), 1);
        bench.stop().await;
    }

    /// A claim goes with its program: killed (no release, no disconnect) or past its ttl.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn claim_released_when_holder_killed_or_ttl() {
        let bench = start("claim-gone", Duration::from_secs(30));
        let b = bench.program("bob").await;
        let mut events = b.hub.registry().events().call().await.unwrap();

        // a runs on a runtime of its own, dropped with everything on it, as a killed process would be
        let (socket, key) = (bench.socket(), bench.key.clone());
        let (claimed_tx, claimed_rx) = std::sync::mpsc::channel();
        let (kill_tx, kill_rx) = std::sync::mpsc::channel::<()>();
        let a = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                let meta = ClientMeta::new().program("alice");
                let hub =
                    HubClient::config(|c| c.hub_api_at(socket.clone()).client_meta(meta.clone()))
                        .connect()
                        .await
                        .unwrap();
                let dev = Client::config(|c| {
                    c.hub_device_at(socket.clone(), key.clone())
                        .client_meta(meta.clone())
                })
                .connect()
                .await
                .unwrap();
                let id = hub
                    .claims()
                    .claim(key.clone(), bus0(), "flashing".into(), None)
                    .call()
                    .await
                    .unwrap();
                claimed_tx.send(id).unwrap();
                std::mem::forget(hub);
                std::mem::forget(dev);
            });
            kill_rx.recv().unwrap();
            rt.shutdown_background();
        });
        let id = claimed_rx.recv().unwrap();
        assert_eq!(refused(b.dev.bus(0).send(1).call().await), in_use("alice"));
        kill_tx.send(()).unwrap();
        a.join().unwrap();
        loop {
            let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
                .await
                .expect("an event")
                .unwrap();
            match event {
                EventOwned::Released { id: gone, .. } => {
                    assert_eq!(gone, id);
                    break;
                }
                _ => continue,
            }
        }
        assert_eq!(b.dev.bus(0).send(1).call().await.unwrap(), 1);

        // a ttl: gone after it, without anyone asking
        let c = bench.program("carol").await;
        bench.claim(&c, bus0(), Some(1)).await;
        assert_eq!(refused(b.dev.bus(0).send(2).call().await), in_use("carol"));
        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert_eq!(b.dev.bus(0).send(2).call().await.unwrap(), 2);
        assert!(
            b.hub
                .claims()
                .claims(bench.key.clone())
                .call()
                .await
                .unwrap()
                .is_empty()
        );
        bench.stop().await;
    }

    // ---- device-wide exclusive and lending (SRV-32) ----

    /// Waits for the registry event `f` picks, skipping others.
    async fn next_event<T>(
        events: &mut wire_weaver_client::Stream<EventOwned>,
        f: impl Fn(&EventOwned) -> Option<T>,
    ) -> T {
        loop {
            let event = tokio::time::timeout(Duration::from_secs(3), events.recv())
                .await
                .expect("an event")
                .unwrap();
            if let Some(t) = f(&event) {
                return t;
            }
        }
    }

    /// The device's state in the hub's registry, `None` when the hub doesn't hold it.
    async fn hub_state(hub: &HubClient, key: &str) -> Option<ww_hub_api::DeviceStateOwned> {
        hub.registry()
            .devices()
            .call()
            .await
            .unwrap()
            .into_iter()
            .find(|d| d.key == key)
            .map(|d| d.state)
    }

    /// A claims the whole device (a firmware update): B's calls and writes anywhere on it get `Common(InUse)` naming
    /// A, B's reads go on, A's calls go through; a root claim of B is refused while A holds it. Released: B goes on.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn exclusive_refuses_a_second_client() {
        let bench = start("exclusive", Duration::from_secs(30));
        let mut a = bench.program("alice").await;
        let mut b = bench.program("bob").await;
        let id = bench.claim(&a, vec![], None).await;

        assert_eq!(refused(b.dev.bus(0).send(7).call().await), in_use("alice"));
        assert_eq!(refused(b.dev.bus(1).send(7).call().await), in_use("alice"));
        assert_eq!(
            refused(b.dev.bus(1).write_speed(1).write().await),
            in_use("alice")
        );
        // `add` has no error type: an error event names the holder
        match b.dev.add(1, 2).call().await {
            Err(wire_weaver_client::Error::RemoteError(e)) => assert!(
                matches!(&e.kind, ww_client_server::ErrorKindOwned::UserStr(s) if *s == format!("in use by {}", by("alice"))),
                "{e:?}"
            ),
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert_eq!(b.dev.bus(1).read_speed().read().await.unwrap(), 0);
        assert_eq!(a.dev.add(1, 2).call().await.unwrap(), 3);
        assert_eq!(a.dev.bus(1).send(5).call().await.unwrap(), 5);
        match bench.claim_refused(&b, vec![]).await {
            ClaimErrorOwned::InUse { claim } => assert_eq!(claim.id, id),
            other => panic!("expected InUse, got {other:?}"),
        }

        a.hub.claims().release(id).call().await.unwrap();
        assert_eq!(b.dev.add(1, 2).call().await.unwrap(), 3);
        bench.stop().await;
    }

    /// A root claim with a ttl goes by itself when it runs out, told by a `Released` event without anyone asking;
    /// one of a program that is killed goes with it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn exclusive_ends_at_ttl_and_with_its_holder() {
        let bench = start("exclusive-gone", Duration::from_secs(30));
        let b = bench.program("bob").await;
        let mut events = b.hub.registry().events().call().await.unwrap();
        let a = bench.program("alice").await;
        let id = bench.claim(&a, vec![], Some(1)).await;
        assert_eq!(refused(b.dev.bus(0).send(1).call().await), in_use("alice"));
        let gone = next_event(&mut events, |e| match e {
            EventOwned::Released { id, .. } => Some(*id),
            _ => None,
        })
        .await;
        assert_eq!(gone, id);
        assert_eq!(b.dev.bus(0).send(1).call().await.unwrap(), 1);

        // killed: its runtime dropped with everything on it, no release, no disconnect
        let (socket, key) = (bench.socket(), bench.key.clone());
        let (claimed_tx, claimed_rx) = std::sync::mpsc::channel();
        let (kill_tx, kill_rx) = std::sync::mpsc::channel::<()>();
        let c = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                let meta = ClientMeta::new().program("carol");
                let hub =
                    HubClient::config(|c| c.hub_api_at(socket.clone()).client_meta(meta.clone()))
                        .connect()
                        .await
                        .unwrap();
                let id = hub
                    .claims()
                    .claim(key.clone(), vec![], "flashing".into(), None)
                    .call()
                    .await
                    .unwrap();
                claimed_tx.send(id).unwrap();
                std::mem::forget(hub);
            });
            kill_rx.recv().unwrap();
            rt.shutdown_background();
        });
        let id = claimed_rx.recv().unwrap();
        assert_eq!(refused(b.dev.bus(0).send(2).call().await), in_use("carol"));
        kill_tx.send(()).unwrap();
        c.join().unwrap();
        let gone = next_event(&mut events, |e| match e {
            EventOwned::Released { id, .. } => Some(*id),
            _ => None,
        })
        .await;
        assert_eq!(gone, id);
        assert_eq!(b.dev.bus(0).send(2).call().await.unwrap(), 2);
        drop(a);
        bench.stop().await;
    }

    /// `ww hub lend`'s round trip: the hub closes the device for another program (its tunnels end, new ones are
    /// refused as `Lent`), which then has it to itself; returned, the hub opens it again and clients come back.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn lend_round_trip() {
        let bench = start("lend", Duration::from_secs(30));
        let mut a = bench.client().await;
        assert_eq!(a.add(1, 1).call().await.unwrap(), 2);
        let lender = HubClient::config(|c| c.hub_api_at(bench.socket()))
            .connect()
            .await
            .unwrap();
        let mut events = lender.registry().events().call().await.unwrap();
        assert_eq!(
            hub_state(&lender, &bench.key).await,
            Some(ww_hub_api::DeviceStateOwned::Open)
        );

        let id = lender
            .claims()
            .lend(bench.key.clone(), "probe-rs".into(), None)
            .call()
            .await
            .unwrap();
        // closed when lend answers (a's tunnel got `Disconnect(Other(DISCONNECT_LENT))`), not held by the hub
        assert_eq!(hub_state(&lender, &bench.key).await, None);
        let claims = lender
            .claims()
            .claims(bench.key.clone())
            .call()
            .await
            .unwrap();
        assert_eq!((claims[0].id, claims[0].lent), (id, true));
        let lent = next_event(&mut events, |e| match e {
            EventOwned::Lent { id, .. } => Some(*id),
            _ => None,
        })
        .await;
        assert_eq!(lent, id);

        // a client through the hub is refused, saying why; the other program opens the device itself
        let (socket, key) = (bench.socket(), bench.key.clone());
        let refused = Client::config(|c| c.hub_device_at(socket.clone(), key.clone()))
            .connect()
            .await
            .err()
            .expect("refused while lent");
        assert!(
            format!("{refused:#}").contains("lent"),
            "says why: {refused:#}"
        );
        let other = HubClient::config(|c| c.hub_api_at(bench.socket()))
            .connect()
            .await
            .unwrap();
        for path in [vec![], bus0()] {
            match other
                .claims()
                .claim(bench.key.clone(), path, String::new(), None)
                .call()
                .await
                .map_err(device_error)
            {
                Err(ClaimErrorOwned::InUse { claim }) => assert!(claim.lent),
                r => panic!("expected InUse naming the lend, got {r:?}"),
            }
        }
        let mut direct = Client::config(|c| c.in_process_path(bench.device.path()))
            .connect()
            .await
            .expect("the device is free for the other program");
        assert_eq!(direct.add(2, 2).call().await.unwrap(), 4);
        direct.cmd.disconnect().asynch().await.unwrap();

        lender.claims().release(id).call().await.unwrap();
        next_event(&mut events, |e| {
            matches!(e, EventOwned::Returned { .. }).then_some(())
        })
        .await;
        // the hub held it before: open again
        next_event(&mut events, |e| {
            matches!(e, EventOwned::Opened { .. }).then_some(())
        })
        .await;
        let mut b = bench.client().await;
        assert_eq!(b.add(3, 3).call().await.unwrap(), 6);
        bench.stop().await;
    }

    /// A borrower that dies returns the device: the lend is a claim of its client instance.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn lend_returned_when_borrower_killed() {
        let bench = start("lend-killed", Duration::from_secs(30));
        let mut a = bench.client().await;
        assert_eq!(a.add(1, 1).call().await.unwrap(), 2);
        let watcher = HubClient::config(|c| c.hub_api_at(bench.socket()))
            .connect()
            .await
            .unwrap();
        let mut events = watcher.registry().events().call().await.unwrap();

        let (socket, key) = (bench.socket(), bench.key.clone());
        let (lent_tx, lent_rx) = std::sync::mpsc::channel();
        let (kill_tx, kill_rx) = std::sync::mpsc::channel::<()>();
        let borrower = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                let hub = HubClient::config(|c| c.hub_api_at(socket.clone()))
                    .connect()
                    .await
                    .unwrap();
                let id = hub
                    .claims()
                    .lend(key.clone(), "dfu-util".into(), None)
                    .call()
                    .await
                    .unwrap();
                lent_tx.send(id).unwrap();
                std::mem::forget(hub);
            });
            kill_rx.recv().unwrap();
            rt.shutdown_background();
        });
        lent_rx.recv().unwrap();
        assert_eq!(hub_state(&watcher, &bench.key).await, None);
        kill_tx.send(()).unwrap();
        borrower.join().unwrap();
        next_event(&mut events, |e| {
            matches!(e, EventOwned::Returned { .. }).then_some(())
        })
        .await;
        next_event(&mut events, |e| {
            matches!(e, EventOwned::Opened { .. }).then_some(())
        })
        .await;
        let mut b = bench.client().await;
        assert_eq!(b.add(2, 3).call().await.unwrap(), 5);
        bench.stop().await;
    }

    /// `hub::Exclusive`, as `ww dfu` takes it: nothing while the hub doesn't hold the device; once it does, the
    /// program's own device connection (same `ClientMeta`) goes on and another program's is refused naming it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn exclusive_helper_through_the_hub() {
        use wire_weaver_client::hub::Exclusive;
        let bench = start("exclusive-helper", Duration::from_secs(30));
        let socket = PathBuf::from(bench.socket());
        let meta = ClientMeta::new().program("ww dfu");
        assert!(
            Exclusive::take_at(&socket, &bench.key, &meta, "ww dfu flash")
                .await
                .unwrap()
                .is_none(),
            "the hub doesn't hold it"
        );
        let own = bench.client_with(|c| c.client_meta(meta.clone())).await;
        let other = bench.program("bob").await;
        let exclusive = Exclusive::take_at(&socket, &bench.key, &meta, "ww dfu flash")
            .await
            .unwrap()
            .expect("taken");
        assert_eq!(own.bus(0).send(1).call().await.unwrap(), 1);
        assert_eq!(
            refused(other.dev.bus(0).send(1).call().await),
            in_use("ww dfu")
        );
        // a second one is refused, saying by whom and why
        let e = Exclusive::take_at(&socket, &bench.key, &ClientMeta::new(), "again")
            .await
            .err()
            .expect("refused");
        assert!(format!("{e:#}").contains("(ww dfu flash)"), "{e:#}");
        exclusive.release().await.unwrap();
        assert_eq!(other.dev.bus(0).send(1).call().await.unwrap(), 1);
        bench.stop().await;
    }
}

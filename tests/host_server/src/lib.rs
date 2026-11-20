//! The host server (SRV-12, `wire_weaver_server`): many clients on one API, each connection with its own backend,
//! limits and shutdown. Clients and server run in one process, on tokio's clock.

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tokio::sync::{Notify, oneshot};
    use tokio::task::JoinHandle;
    use wire_weaver::prelude::*;
    use wire_weaver::{Gid, Peer, Pid, Uid};
    use wire_weaver_client::in_process::TokioClock;
    use wire_weaver_client::{Commander, WwClient};
    use wire_weaver_server::in_process::InProcessListener;
    use wire_weaver_server::{Connection, Limits, Server};

    /// What every connection's backend shares.
    #[derive(Default)]
    struct Shared {
        gates: Mutex<[bool; 4]>,
        opened: Notify,
        /// Handlers inside `wait`.
        waiting: AtomicU32,
        /// The connections the factory made backends for.
        connections: Mutex<Vec<Connection>>,
    }

    impl Shared {
        fn open(&self, gate: u8) {
            self.gates.lock().unwrap()[gate as usize] = true;
            self.opened.notify_waiters();
        }

        async fn gate(&self, gate: u8) {
            loop {
                let opened = self.opened.notified();
                tokio::pin!(opened);
                opened.as_mut().enable();
                if self.gates.lock().unwrap()[gate as usize] {
                    return;
                }
                opened.await;
            }
        }

        /// Until `count` handlers are inside `wait`.
        async fn waiting(&self, count: u32) {
            while self.waiting.load(Ordering::Acquire) != count {
                tokio::task::yield_now().await;
            }
        }
    }

    /// One connection's server.
    struct Backend {
        shared: Arc<Shared>,
        id: u32,
    }

    impl Backend {
        async fn echo(&mut self, _cx: &mut Context<'_, impl EventOut>, x: u32) -> RpcResult<u32> {
            Ready(x)
        }

        async fn wait(&mut self, _cx: &mut Context<'_, impl EventOut>, gate: u8) -> RpcResult<u32> {
            self.shared.waiting.fetch_add(1, Ordering::AcqRel);
            self.shared.gate(gate).await;
            self.shared.waiting.fetch_sub(1, Ordering::AcqRel);
            Ready(u32::from(gate))
        }

        async fn whoami(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<u32> {
            Ready(self.id)
        }

        async fn burst(
            &mut self,
            cx: &mut Context<'_, impl EventOut>,
            count: u32,
        ) -> RpcResult<u32> {
            for n in 0..count {
                api_impl::stream_data_ser()
                    .numbers_send(&n, cx)
                    .await
                    .expect("the item is queued");
            }
            Ready(count)
        }

        async fn big(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            len: u32,
        ) -> RpcResult<Vec<u8>> {
            Ready(vec![7; len as usize])
        }

        async fn numbers(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
            Ready(())
        }

        async fn close_numbers(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

        async fn sideband_numbers(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            _sideband: ww_client_server::StreamSideband,
        ) -> Option<ww_client_server::StreamSideband> {
            None
        }
    }

    impl WireWeaverAsyncApiBackend for Backend {
        type Medium = ();

        async fn process_bytes<'a>(
            &mut self,
            out: &mut EventWriter<'_, impl MessageSink>,
            medium: (),
            data: &[u8],
            scratch: &'a mut [u8],
        ) -> Result<&'a [u8], ShrinkWrapError> {
            self.process_request_bytes(data, scratch, out, medium).await
        }

        fn version(&self) -> FullVersion<'_> {
            host_server_api::HOST_SERVER_FULL_GID
        }
    }

    mod api_impl {
        wire_weaver::ww_codegen!(
            host_server_api :: HostServer for super::Backend,
            server = true, no_alloc = false, use_async = true,
            method_model = "_=immediate",
            property_model = "_=get_set",
            introspect = "no_docs",
        );
    }

    struct Client {
        cmd: Commander,
    }

    impl WwClient for Client {
        fn default_config() -> wire_weaver_client::ClientConfig {
            wire_weaver_client::ClientConfig::default()
        }

        fn try_from_cmd(cmd: Commander) -> Result<Self, wire_weaver_client::Error> {
            Ok(Self { cmd })
        }
    }

    mod api_client {
        wire_weaver::ww_codegen!(
            host_server_api :: HostServer for super::Client,
            client = "std_client",
        );
    }

    /// A server on the test's runtime, its shutdown in the test's hand.
    struct Bench {
        path: String,
        shared: Arc<Shared>,
        shutdown: Option<oneshot::Sender<()>>,
        served: JoinHandle<()>,
    }

    impl Bench {
        fn start(path: &str, limits: Limits) -> Self {
            Self::start_on(InProcessListener::new(path, MESSAGE_LEN_MAX), path, limits)
        }

        fn start_on(listener: InProcessListener, path: &str, limits: Limits) -> Self {
            tests_common::api!(api_impl).seed();
            let shared = Arc::new(Shared::default());
            let (shutdown, asked) = oneshot::channel();
            let link = ww_device::LinkConfig::new(
                host_server_api::HOST_SERVER_FULL_GID,
                host_server_api::HOST_SERVER_WIRE_VERSION,
                api_impl::api_hash(),
                api_impl::image_facts(),
                ww_client_server::COMPACT_VERSION,
            );
            let for_backends = shared.clone();
            let served = tokio::spawn(async move {
                let make = move |connection: &Connection| {
                    for_backends
                        .connections
                        .lock()
                        .unwrap()
                        .push(connection.clone());
                    Backend {
                        shared: for_backends.clone(),
                        id: connection.id.0,
                    }
                };
                Server::new(TokioClock::new())
                    .limits(limits)
                    .serve(link, listener, make, async {
                        let _ = asked.await;
                    })
                    .await
                    .unwrap();
            });
            Bench {
                path: path.to_string(),
                shared,
                shutdown: Some(shutdown),
                served,
            }
        }

        fn connect(&self) -> impl Future<Output = Client> + Send + use<> {
            let path = self.path.clone();
            async move {
                Client::config(move |c| {
                    c.in_process_path(path.clone())
                        .resolver(tests_common::resolver())
                })
                .connect()
                .await
                .expect("connect")
            }
        }

        fn shut_down(&mut self) {
            self.shutdown.take().unwrap().send(()).unwrap();
        }
    }

    /// A hang is what these tests look for: name it.
    async fn soon<T>(what: &str, f: impl Future<Output = T>) -> T {
        tokio::time::timeout(Duration::from_secs(20), f)
            .await
            .unwrap_or_else(|_| panic!("{what}: still waiting"))
    }

    /// The longest message the test servers take.
    const MESSAGE_LEN_MAX: usize = 4096;

    fn small() -> Limits {
        Limits::default()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fifty_clients_at_once() {
        let bench = Bench::start("host_server/fifty", small());
        let clients: Vec<_> = (0..50u32)
            .map(|n| {
                let connect = bench.connect();
                tokio::spawn(async move {
                    let client = connect.await;
                    for round in 0..20 {
                        assert_eq!(
                            client.echo(n * 100 + round).call().await.unwrap(),
                            n * 100 + round
                        );
                    }
                    client.whoami().call().await.unwrap()
                })
            })
            .collect();
        let mut ids = vec![];
        for client in clients {
            ids.push(soon("a client", client).await.unwrap());
        }
        ids.sort_unstable();
        assert_eq!(
            ids,
            (0..50).collect::<Vec<_>>(),
            "each connection has its own backend"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_slow_handler_holds_up_its_own_connection_only() {
        let bench = Bench::start("host_server/slow", small());
        let slow = Arc::new(bench.connect().await);
        let other = bench.connect().await;
        let waiting = tokio::spawn({
            let slow = slow.clone();
            async move { slow.wait(1).call().await.unwrap() }
        });
        soon("the handler runs", bench.shared.waiting(1)).await;
        // the same connection's next request waits its turn, another connection's does not
        let behind = tokio::spawn({
            let slow = slow.clone();
            async move { slow.echo(7).call().await.unwrap() }
        });
        for n in 0..10 {
            assert_eq!(
                soon("the other client", other.echo(n).call())
                    .await
                    .unwrap(),
                n
            );
        }
        assert!(!waiting.is_finished() && !behind.is_finished());
        bench.shared.open(1);
        assert_eq!(soon("the slow call", waiting).await.unwrap(), 1);
        assert_eq!(soon("the call behind it", behind).await.unwrap(), 7);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn clients_past_the_connection_limit_wait_for_room() {
        let limits = Limits {
            connections_max: 2,
            ..small()
        };
        let bench = Bench::start("host_server/limit", limits);
        let first = bench.connect().await;
        let second = bench.connect().await;
        let third = tokio::spawn(bench.connect());
        // both are served while the third waits: give it every chance to get in
        for n in 0..50 {
            assert_eq!(first.echo(n).call().await.unwrap(), n);
            assert_eq!(second.echo(n).call().await.unwrap(), n);
        }
        assert!(!third.is_finished());
        assert_eq!(bench.shared.connections.lock().unwrap().len(), 2);
        drop(first);
        let third = soon("the third client", third).await.unwrap();
        assert_eq!(third.whoami().call().await.unwrap(), 2);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_handler_that_sends_more_than_the_outbox_holds_waits_for_the_client() {
        let limits = Limits {
            events_max: 4,
            ..small()
        };
        let bench = Bench::start("host_server/burst", limits);
        let client = bench.connect().await;
        let mut numbers = client.numbers().call().await.unwrap();
        let burst = client.burst(300).call();
        let read = async {
            for n in 0..300 {
                assert_eq!(numbers.recv().await.unwrap(), n);
            }
        };
        let (count, ()) = soon("the burst", async { tokio::join!(burst, read) }).await;
        assert_eq!(count.unwrap(), 300);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_reply_over_the_links_limit_is_an_error_not_a_timeout() {
        let bench = Bench::start("host_server/big", small());
        let client = bench.connect().await;
        assert_eq!(client.big(3000).call().await.unwrap().len(), 3000);
        let error = soon(
            "the long reply",
            client.big(3 * MESSAGE_LEN_MAX as u32).call(),
        )
        .await
        .unwrap_err();
        let wire_weaver::call::ErrorKind::MessageTooBig(too_big) = error.call_kind() else {
            panic!("expected a message over the limit, got {error:?}");
        };
        assert_eq!(too_big.limit.get() as usize, MESSAGE_LEN_MAX);
        // the connection goes on
        assert_eq!(client.echo(1).call().await.unwrap(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_peer_and_the_clients_own_words_reach_the_factory() {
        let peer = Peer::Unix {
            uid: Uid(1000),
            gid: Gid(100),
            pid: Some(Pid(4242)),
        };
        let path = "host_server/peer";
        let listener = InProcessListener::new(path, MESSAGE_LEN_MAX).with_peer(peer);
        let bench = Bench::start_on(listener, path, small());
        let client = bench.connect().await;
        assert_eq!(client.whoami().call().await.unwrap(), 0);
        let connections = bench.shared.connections.lock().unwrap();
        assert_eq!(connections[0].peer, peer);
        assert!(!connections[0].client.program.is_empty());
        assert_eq!(connections[0].message_len_max, MESSAGE_LEN_MAX);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_answers_what_is_in_flight_then_disconnects() {
        let mut bench = Bench::start("host_server/drain", small());
        let client = Arc::new(bench.connect().await);
        let idle = bench.connect().await;
        let in_flight = tokio::spawn({
            let client = client.clone();
            async move { client.wait(2).call().await }
        });
        soon("the handler runs", bench.shared.waiting(1)).await;
        bench.shut_down();
        // an idle connection is disconnected at once, nobody new gets in
        soon("the idle client is told", async {
            while idle.echo(1).call().await.is_ok() {
                tokio::task::yield_now().await;
            }
        })
        .await;
        assert!(
            !bench.served.is_finished(),
            "the server waits for the request in flight"
        );
        bench.shared.open(2);
        assert_eq!(
            soon("the call in flight", in_flight)
                .await
                .unwrap()
                .unwrap(),
            2
        );
        soon("the server ends", &mut bench.served).await.unwrap();
        assert!(client.echo(1).call().await.is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn after_the_grace_time_a_handler_that_never_returns_is_dropped() {
        let limits = Limits {
            shutdown_grace: Duration::from_secs(3),
            ..small()
        };
        let mut bench = Bench::start("host_server/grace", limits);
        let client = Arc::new(bench.connect().await);
        let stuck = tokio::spawn({
            let client = client.clone();
            async move { client.wait(3).call().await }
        });
        bench.shared.waiting(1).await;
        let asked_at = tokio::time::Instant::now();
        bench.shut_down();
        (&mut bench.served).await.unwrap();
        assert_eq!(asked_at.elapsed(), Duration::from_secs(3));
        assert!(stuck.await.unwrap().is_err());
    }
}

//! The host server (SRV-12, `wire_weaver_server`): many clients on one API, each connection with its own backend,
//! limits and shutdown. Clients and server run in one process, on tokio's clock. Byte-stream listeners (TR-19): TCP,
//! streams handed in, two APIs on one source; stdio in `tests/stdio.rs` with the `stdio_server` binary. UDP (SRV-69):
//! a connection per host of one datagram socket.

pub mod counter;

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use futures_util::StreamExt;
    use tokio::sync::{Notify, oneshot};
    use tokio::task::JoinHandle;
    use wire_weaver::prelude::*;
    use wire_weaver::{CallContext, DisconnectReason, Gid, Peer, Pid, Uid};
    use wire_weaver_client::in_process::TokioClock;
    use wire_weaver_client::{Commander, WwClient};
    use wire_weaver_server::byte_stream::{PreambleFraming, listen};
    use wire_weaver_server::in_process::InProcessListener;
    use wire_weaver_server::local_socket::LocalSocketIncoming;
    use wire_weaver_server::{
        Access, Connection, Denied, Fanout, Limits, Pending, PumpId, Pumps, Resource, Server,
        Subscription, Verdict, from_fn,
    };
    use ww_device::Instant;
    use ww_stream::observe::ObserveLimits;

    /// What every connection's backend shares.
    struct Shared {
        gates: Mutex<[bool; 4]>,
        opened: Notify,
        /// Handlers inside `wait`.
        waiting: AtomicU32,
        /// The connections the factory made backends for.
        connections: Mutex<Vec<Connection>>,
        news: Fanout<u32>,
        mode: Mutex<u8>,
        mode_changed: Fanout<()>,
        /// Backends not dropped yet: connections that run.
        live: AtomicU32,
        /// What became of backends, in order: `end` ran (`End`), dropped (`Drop`), by connection id.
        ends: Mutex<Vec<(Ending, u32)>>,
        /// `end` never returns.
        end_hangs: std::sync::atomic::AtomicBool,
        /// A backend was dropped.
        dropped: Notify,
    }

    /// What became of a backend.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Ending {
        End,
        Drop,
    }

    impl Default for Shared {
        fn default() -> Self {
            Shared {
                gates: Mutex::default(),
                opened: Notify::new(),
                waiting: AtomicU32::new(0),
                connections: Mutex::default(),
                news: Fanout::new(8),
                mode: Mutex::default(),
                mode_changed: Fanout::new(1),
                live: AtomicU32::new(0),
                ends: Mutex::default(),
                end_hangs: std::sync::atomic::AtomicBool::new(false),
                dropped: Notify::new(),
            }
        }
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

        /// Until the backend of connection `id` was dropped; what became of it.
        async fn ended(&self, id: u32) -> Vec<Ending> {
            loop {
                let dropped = self.dropped.notified();
                tokio::pin!(dropped);
                dropped.as_mut().enable();
                let ends: Vec<_> = {
                    let ends = self.ends.lock().unwrap();
                    ends.iter()
                        .filter(|(_, of)| *of == id)
                        .map(|(e, _)| *e)
                        .collect()
                };
                if ends.contains(&Ending::Drop) {
                    return ends;
                }
                dropped.await;
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
        /// The connection's pumps: news flows through one while this client has the stream open.
        pumps: Pumps,
        mode_changes: Subscription<()>,
        /// Another connection wrote `mode`.
        mode_changed: bool,
        observe: api_impl::ObserveState,
        /// Updates of observed properties are built here.
        update_buf: Vec<u8>,
    }

    /// Counts a handler in `Shared::waiting` while it runs, also when the server drops it (SRV-47).
    struct Waiting<'a>(&'a AtomicU32);

    impl<'a> Waiting<'a> {
        fn new(count: &'a AtomicU32) -> Self {
            count.fetch_add(1, Ordering::AcqRel);
            Waiting(count)
        }
    }

    impl Drop for Waiting<'_> {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::AcqRel);
        }
    }

    impl Drop for Backend {
        fn drop(&mut self) {
            self.shared.live.fetch_sub(1, Ordering::AcqRel);
            self.shared
                .ends
                .lock()
                .unwrap()
                .push((Ending::Drop, self.id));
            self.shared.dropped.notify_waiters();
        }
    }

    /// The pump of the `news` stream.
    const NEWS: PumpId = PumpId::new(1);

    impl wire_weaver_server::Backend for Backend {
        /// A write to `mode` by any client.
        async fn wait(&mut self) {
            std::future::poll_fn(|cx| {
                let mut woke = false;
                while let std::task::Poll::Ready(Some(_)) = self.mode_changes.poll_recv(cx) {
                    self.mode_changed = true;
                    woke = true;
                }
                if woke {
                    std::task::Poll::Ready(())
                } else {
                    std::task::Poll::Pending
                }
            })
            .await;
        }

        fn next_wake(&mut self, now: Instant) -> Option<Instant> {
            self.observe
                .next_wake(now.as_micros())
                .map(Instant::from_micros)
        }

        async fn poll(
            &mut self,
            out: &mut EventWriter<'_, impl MessageSink>,
            medium: (),
            now: Instant,
        ) {
            if std::mem::take(&mut self.mode_changed) {
                self.mark_changed_mode();
            }
            let mut buf = std::mem::take(&mut self.update_buf);
            let _ = self
                .poll_properties(out, medium, now.as_micros(), &mut buf)
                .await;
            self.update_buf = buf;
        }

        async fn end(&mut self) {
            if self.shared.end_hangs.load(Ordering::Acquire) {
                std::future::pending::<()>().await;
            }
            self.shared
                .ends
                .lock()
                .unwrap()
                .push((Ending::End, self.id));
        }
    }

    impl Backend {
        async fn echo(&self, _cx: &mut Context<'_, impl EventOut>, x: u32) -> RpcResult<u32> {
            Ready(x)
        }

        async fn wait(&self, _cx: &mut Context<'_, impl EventOut>, gate: u8) -> RpcResult<u32> {
            let _waiting = Waiting::new(&self.shared.waiting);
            self.shared.gate(gate).await;
            Ready(u32::from(gate))
        }

        async fn hold(&mut self, _cx: &mut Context<'_, impl EventOut>, gate: u8) -> RpcResult<u32> {
            let _waiting = Waiting::new(&self.shared.waiting);
            self.shared.gate(gate).await;
            Ready(u32::from(gate))
        }

        async fn call_deadline(
            &self,
            cx: &mut Context<'_, impl EventOut>,
        ) -> RpcResult<Option<ww_si::Millis<u32>>> {
            Ready(cx.call().deadline)
        }

        async fn call_trace(&self, cx: &mut Context<'_, impl EventOut>) -> RpcResult<Option<u8>> {
            Ready(cx.call().trace.map(|trace| trace.trace_id.0[0]))
        }

        async fn whoami(&self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<u32> {
            Ready(self.id)
        }

        async fn call_connection(&self, cx: &mut Context<'_, impl EventOut>) -> RpcResult<u32> {
            Ready(cx.call().connection.0)
        }

        async fn call_uid(&self, cx: &mut Context<'_, impl EventOut>) -> RpcResult<u32> {
            match cx.call().peer {
                Peer::Unix { uid, .. } => Ready(uid.0),
                _ => Ready(0),
            }
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

        async fn news(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
            let items = self
                .shared
                .news
                .subscribe()
                .filter_map(|item| std::future::ready(item.ok()));
            self.pumps.add(NEWS, items, |item, buf| {
                api_impl::stream_data_ser().news(item, buf).map(<[u8]>::len)
            });
            Ready(())
        }

        async fn close_news(&mut self, _cx: &mut Context<'_, impl EventOut>) {
            self.pumps.remove(NEWS);
        }

        async fn sideband_news(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            _sideband: ww_client_server::StreamSideband,
        ) -> Option<ww_client_server::StreamSideband> {
            None
        }

        async fn set_mode(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            value: u8,
        ) -> SetResult<CommonError<'_>> {
            *self.shared.mode.lock().unwrap() = value;
            self.shared.mode_changed.send(());
            Set
        }

        async fn get_mode(
            &self,
            _cx: &mut Context<'_, impl EventOut>,
        ) -> GetResult<u8, CommonError<'_>> {
            Value(*self.shared.mode.lock().unwrap())
        }

        fn observe_state(&mut self) -> &mut api_impl::ObserveState {
            &mut self.observe
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

    impl wire_weaver::WireWeaverSharedApiBackend for Backend {
        async fn process_exclusive_bytes<'a>(
            &mut self,
            origin: &wire_weaver::CallContext,
            out: &mut EventWriter<'_, impl MessageSink>,
            medium: (),
            data: &[u8],
            scratch: &'a mut [u8],
        ) -> Result<&'a [u8], ShrinkWrapError> {
            self.process_request_from(origin, data, scratch, out, medium)
                .await
        }

        fn is_shared(data: &[u8]) -> bool {
            Backend::is_shared(data)
        }

        async fn process_shared_bytes<'a>(
            &self,
            origin: &wire_weaver::CallContext,
            out: &mut EventWriter<'_, impl MessageSink>,
            medium: (),
            data: &[u8],
            scratch: &'a mut [u8],
        ) -> Result<&'a [u8], ShrinkWrapError> {
            self.process_shared_from(origin, data, scratch, out, medium)
                .await
        }
    }

    mod api_impl {
        wire_weaver::ww_codegen!(
            host_server_api :: HostServer for super::Backend,
            server = true, no_alloc = false, use_async = true,
            method_model = "_=immediate",
            property_model = "_=get_set",
            receiver_model = "echo|wait|whoami|call_connection|call_uid|call_deadline|call_trace|mode=shared",
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

    /// How clients reach a [Bench].
    #[derive(Clone)]
    enum Reach {
        InProcess(String),
        LocalSocket(String),
        Tcp(String),
        Stream(wire_weaver_client::StreamOpener),
        WebSocket(String),
        Udp(String),
    }

    /// Start a [Bench] on `$listener`. A macro, not a function generic over the listener: the futures of a
    /// generic listener's halves can't be shown to be `Send`, a concrete one's can.
    macro_rules! start_bench {
        ($listener:expr, $reach:expr, $limits:expr, $as_tasks:expr $(,)?) => {
            start_bench!($listener, $reach, $limits, $as_tasks, |server: Server<
                TokioClock,
            >| server)
        };
        // `configure`: what the test sets on the server besides its limits, its authorization
        ($listener:expr, $reach:expr, $limits:expr, $as_tasks:expr, $configure:expr $(,)?) => {{
            let (listener, reach, limits, as_tasks) = ($listener, $reach, $limits, $as_tasks);
            let configure = $configure;
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
                    for_backends.live.fetch_add(1, Ordering::AcqRel);
                    Backend {
                        shared: for_backends.clone(),
                        id: connection.id.0,
                        pumps: connection.pumps.clone(),
                        mode_changes: for_backends.mode_changed.subscribe(),
                        mode_changed: false,
                        observe: api_impl::ObserveState::new(ObserveLimits::DEFAULT),
                        update_buf: vec![0; MESSAGE_LEN_MAX],
                    }
                };
                let server = configure(Server::new(TokioClock::new()).limits(limits));
                let shutdown = async { asked.await.unwrap_or(DisconnectReason::Shutdown) };
                if as_tasks {
                    // every connection a task of its own, on any of the runtime's threads
                    let mut accepting = server.accepting(link, listener, make, shutdown);
                    while let Some(connection) = accepting.next().await.unwrap() {
                        tokio::spawn(connection);
                    }
                } else {
                    server.serve(link, listener, make, shutdown).await.unwrap();
                }
            });
            Bench {
                reach,
                shared,
                shutdown: Some(shutdown),
                served,
            }
        }};
    }

    /// A server on the test's runtime, its shutdown in the test's hand.
    struct Bench {
        reach: Reach,
        shared: Arc<Shared>,
        shutdown: Option<oneshot::Sender<DisconnectReason>>,
        served: JoinHandle<()>,
    }

    impl Bench {
        fn start(path: &str, limits: Limits) -> Self {
            Self::start_on(InProcessListener::new(path, MESSAGE_LEN_MAX), path, limits)
        }

        fn start_on(listener: InProcessListener, path: &str, limits: Limits) -> Self {
            start_bench!(listener, Reach::InProcess(path.to_owned()), limits, false)
        }

        /// The connections as tasks of their own.
        fn start_as_tasks(path: &str, limits: Limits) -> Self {
            start_bench!(
                InProcessListener::new(path, MESSAGE_LEN_MAX),
                Reach::InProcess(path.to_owned()),
                limits,
                true,
            )
        }

        /// On the local socket `socket`, which the server listens on.
        fn start_local(socket: ww_local_socket::Listener, path: &std::path::Path) -> Self {
            start_bench!(
                listen(
                    LocalSocketIncoming::new(socket),
                    PreambleFraming::new("host server 0.1.0", MESSAGE_LEN_MAX),
                    TokioClock::new(),
                ),
                Reach::LocalSocket(path.to_str().unwrap().to_owned()),
                small(),
                false,
            )
        }

        fn connect(&self) -> impl Future<Output = Client> + Send + use<> {
            let reach = self.reach.clone();
            async move {
                // a client past the connection limit waits in the listener for as long as a test lets it, not the
                // in-process transport's 250 ms
                Client::config(move |c| {
                    let c = match &reach {
                        Reach::InProcess(path) => c.in_process_path(path.clone()),
                        Reach::LocalSocket(path) => c.ipc_path(path.clone()),
                        Reach::Tcp(addr) => c.tcp(addr.clone()),
                        Reach::Stream(opener) => c.stream(opener.clone()),
                        Reach::WebSocket(url) => c.websocket_url(url.clone()),
                        Reach::Udp(addr) => c.udp_addr(addr.clone()),
                    };
                    c.resolver(tests_common::resolver())
                        .link_setup_timeout(SOON)
                })
                .connect()
                .await
                .expect("connect")
            }
        }

        fn shut_down(&mut self) {
            self.shut_down_with(DisconnectReason::Shutdown);
        }

        /// Shut down, telling clients `reason`.
        fn shut_down_with(&mut self, reason: DisconnectReason) {
            self.shutdown.take().unwrap().send(reason).unwrap();
        }
    }

    /// How long a test waits for what it expects before calling it a hang.
    const SOON: Duration = Duration::from_secs(20);

    /// A hang is what these tests look for: name it.
    async fn soon<T>(what: &str, f: impl Future<Output = T>) -> T {
        tokio::time::timeout(SOON, f)
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
    async fn a_slow_handler_that_runs_alone_holds_up_its_own_connection_only() {
        let bench = Bench::start("host_server/slow", small());
        let slow = Arc::new(bench.connect().await);
        let other = bench.connect().await;
        let waiting = tokio::spawn({
            let slow = slow.clone();
            async move { slow.hold(1).call().await.unwrap() }
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

    /// Until `count` handlers wait at their gates.
    async fn waiting(bench: &Bench, count: u32) {
        soon("the handlers run", bench.shared.waiting(count)).await;
    }

    /// Give `pending` every chance to finish while it should not: requests of another connection go through.
    async fn still_pending(bench: &Bench, pending: &[&JoinHandle<u32>]) {
        let other = bench.connect().await;
        for n in 0..20 {
            assert_eq!(other.echo(n).call().await.unwrap(), n);
        }
        for task in pending {
            assert!(!task.is_finished());
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn calls_of_one_connection_run_side_by_side() {
        let bench = Bench::start("host_server/side_by_side", small());
        let client = Arc::new(bench.connect().await);
        let first = tokio::spawn({
            let client = client.clone();
            async move { client.wait(1).call().await.unwrap() }
        });
        let second = tokio::spawn({
            let client = client.clone();
            async move { client.wait(2).call().await.unwrap() }
        });
        waiting(&bench, 2).await;
        // both run at once, and a quick call passes them
        assert_eq!(
            soon("a call beside them", client.echo(3).call())
                .await
                .unwrap(),
            3
        );
        // the later one finishes first
        bench.shared.open(2);
        assert_eq!(soon("the second call", second).await.unwrap(), 2);
        assert!(!first.is_finished());
        bench.shared.open(1);
        assert_eq!(soon("the first call", first).await.unwrap(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_write_waits_for_the_calls_before_it_and_holds_those_after() {
        let bench = Bench::start("host_server/barrier", small());
        let client = Arc::new(bench.connect().await);
        let before = tokio::spawn({
            let client = client.clone();
            async move { client.wait(1).call().await.unwrap() }
        });
        waiting(&bench, 1).await;
        let write = tokio::spawn({
            let client = client.clone();
            async move {
                client.write_mode(5).write().await.unwrap();
                0
            }
        });
        // queued behind the write; give the write time to be read first
        still_pending(&bench, &[&before, &write]).await;
        let after = tokio::spawn({
            let client = client.clone();
            async move { u32::from(client.read_mode().read().await.unwrap()) }
        });
        still_pending(&bench, &[&before, &write, &after]).await;
        // the call before the write finishes: the write runs, then the read after it sees what it wrote
        bench.shared.open(1);
        assert_eq!(soon("the call before", before).await.unwrap(), 1);
        soon("the write", write).await.unwrap();
        assert_eq!(soon("the read after", after).await.unwrap(), 5);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_call_past_its_deadline_is_dropped_and_the_write_behind_it_runs() {
        let bench = Bench::start("host_server/deadline", small());
        let client = Arc::new(bench.connect().await);
        let slow = tokio::spawn({
            let client = client.clone();
            async move {
                client
                    .wait(1)
                    .with_timeout(Duration::from_millis(100))
                    .call()
                    .await
            }
        });
        waiting(&bench, 1).await;
        let write = tokio::spawn({
            let client = client.clone();
            async move { client.write_mode(5).write().await }
        });
        // gate 1 never opens: the call's deadline ends it, and with it the barrier the write waits at
        let error = soon("the slow call", slow).await.unwrap().unwrap_err();
        assert_eq!(
            error.call_kind(),
            wire_weaver::call::ErrorKind::DeadlineExceeded,
            "{error:?}"
        );
        soon("the write", write).await.unwrap().unwrap();
        soon("the handler is dropped", bench.shared.waiting(0)).await;
        assert_eq!(client.read_mode().read().await.unwrap(), 5);
    }

    #[tokio::test(start_paused = true)]
    async fn a_queued_write_past_its_deadline_never_runs() {
        let bench = Bench::start("host_server/queued_deadline", small());
        let client = Arc::new(bench.connect().await);
        let slow = tokio::spawn({
            let client = client.clone();
            async move { client.wait(1).call().await.unwrap() }
        });
        waiting(&bench, 1).await;
        let error = soon(
            "the write",
            client
                .write_mode(5)
                .with_timeout(Duration::from_millis(100))
                .write(),
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.call_kind(),
            wire_weaver::call::ErrorKind::DeadlineExceeded,
            "{error:?}"
        );
        // the server's deadline started when the write reached it, after the client's: once it is over too, the
        // server dropped the write from its queue, and when the call before it finished, nothing writes 5
        tokio::time::sleep(Duration::from_millis(100)).await;
        bench.shared.open(1);
        assert_eq!(soon("the slow call", slow).await.unwrap(), 1);
        assert_eq!(client.read_mode().read().await.unwrap(), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_dropped_call_is_cancelled_on_the_server() {
        let bench = Bench::start("host_server/cancel", small());
        let client = Arc::new(bench.connect().await);
        let running = tokio::spawn({
            let client = client.clone();
            async move { client.wait(1).call().await.unwrap() }
        });
        waiting(&bench, 1).await;
        let queued = tokio::spawn({
            let client = client.clone();
            async move { client.hold(2).call().await.unwrap() }
        });
        still_pending(&bench, &[&running, &queued]).await;
        // the caller of the queued one goes away first: it leaves the queue, its handler never runs
        queued.abort();
        let _ = queued.await;
        running.abort();
        let _ = running.await;
        soon("the running handler is dropped", bench.shared.waiting(0)).await;
        // nothing holds the connection: a call on it is answered, though gates 1 and 2 never open
        assert_eq!(
            soon("a call after", client.echo(3).call()).await.unwrap(),
            3
        );
        assert_eq!(bench.shared.waiting.load(Ordering::Acquire), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_handler_reads_the_callers_deadline_and_trace() {
        let bench = Bench::start("host_server/call_terms", small());
        let client = bench.connect().await;
        assert_eq!(client.call_trace().call().await.unwrap(), None);
        let trace = wire_weaver::TraceContext {
            trace_id: wire_weaver::TraceId([0x4b; 16]),
            parent_id: wire_weaver::SpanId([0x0f; 8]),
            flags: wire_weaver::TraceFlags::SAMPLED,
        };
        let cx = wire_weaver::CallContext::local().with_trace(trace);
        assert_eq!(
            client
                .call_trace()
                .with_call_context(&cx)
                .call()
                .await
                .unwrap(),
            Some(0x4b)
        );
        // the client's timeout is the request's deadline: the handler sees what was left of it
        let left = client
            .call_deadline()
            .with_timeout(SOON)
            .call()
            .await
            .unwrap()
            .expect("a deadline");
        assert!(left.get() <= SOON.as_millis() as u32, "{left:?}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stream_items_flow_while_a_call_runs() {
        let bench = Bench::start("host_server/pump", small());
        let client = Arc::new(bench.connect().await);
        let mut news = client.news().call().await.unwrap();
        let call = tokio::spawn({
            let client = client.clone();
            async move { client.wait(1).call().await.unwrap() }
        });
        waiting(&bench, 1).await;
        for n in 1..=3 {
            bench.shared.news.send(n);
        }
        for n in 1..=3 {
            assert_eq!(
                soon("news while the call runs", news.recv()).await.unwrap(),
                n
            );
        }
        assert!(!call.is_finished());
        bench.shared.open(1);
        assert_eq!(soon("the call", call).await.unwrap(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_request_past_the_servers_cap_is_answered_overloaded() {
        let limits = Limits {
            requests_max: 1,
            ..small()
        };
        let bench = Bench::start("host_server/overloaded", limits);
        let slow = bench.connect().await;
        let other = bench.connect().await;
        let waiting = tokio::spawn(async move { slow.wait(1).call().await.unwrap() });
        soon("the handler runs", bench.shared.waiting(1)).await;
        let error = soon("the refusal", other.echo(5).call()).await.unwrap_err();
        assert_eq!(
            error.call_kind(),
            wire_weaver::call::ErrorKind::Overloaded,
            "{error:?}"
        );
        assert!(!waiting.is_finished());
        bench.shared.open(1);
        assert_eq!(soon("the slow call", waiting).await.unwrap(), 1);
        // room again: the same request is served
        assert_eq!(other.echo(5).call().await.unwrap(), 5);
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
    async fn news_reaches_every_client_that_listens() {
        let bench = Bench::start("host_server/news", small());
        let listeners = [
            bench.connect().await,
            bench.connect().await,
            bench.connect().await,
        ];
        let deaf = bench.connect().await;
        let mut streams = vec![];
        for client in &listeners {
            streams.push(client.news().call().await.unwrap());
        }
        soon("three subscribers", async {
            while bench.shared.news.subscribers() != 3 {
                tokio::task::yield_now().await;
            }
        })
        .await;
        for n in 1..=5 {
            bench.shared.news.send(n);
        }
        for stream in &mut streams {
            for n in 1..=5 {
                assert_eq!(soon("news", stream.recv()).await.unwrap(), n);
            }
        }
        // a client leaves: its subscription goes with its backend
        drop(streams.pop());
        drop(deaf);
        let [_, _, gone] = listeners;
        drop(gone);
        soon("two subscribers", async {
            while bench.shared.news.subscribers() != 2 {
                tokio::task::yield_now().await;
            }
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_write_by_one_client_reaches_the_observers_among_the_others() {
        use wire_weaver_client::observe::on_change;
        let bench = Bench::start("host_server/observe", small());
        let writer = bench.connect().await;
        let watcher = bench.connect().await;
        let mut mode = watcher
            .observe_mode(on_change(Duration::from_millis(1)))
            .subscribe()
            .await
            .unwrap();
        // what it is when the observing starts
        assert_eq!(soon("the first value", mode.recv()).await.unwrap().value, 0);
        writer.write_mode(5).write().await.unwrap();
        assert_eq!(
            soon("the written value", mode.recv()).await.unwrap().value,
            5
        );
        writer.write_mode(6).write().await.unwrap();
        assert_eq!(soon("the next value", mode.recv()).await.unwrap().value, 6);
        assert_eq!(watcher.read_mode().read().await.unwrap(), 6);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn connections_as_tasks_keep_the_limit_and_the_shutdown() {
        let limits = Limits {
            connections_max: 3,
            ..small()
        };
        let mut bench = Bench::start_as_tasks("host_server/tasks", limits);
        let clients = [
            bench.connect().await,
            bench.connect().await,
            bench.connect().await,
        ];
        let fourth = tokio::spawn(bench.connect());
        for (n, client) in clients.iter().enumerate() {
            for _ in 0..20 {
                assert_eq!(client.whoami().call().await.unwrap(), n as u32);
            }
        }
        assert!(!fourth.is_finished(), "the fourth waits for room");
        let [first, second, third] = clients;
        drop(first);
        let fourth = soon("the fourth client", fourth).await.unwrap();
        assert_eq!(fourth.whoami().call().await.unwrap(), 3);
        // a request in flight is answered, then the server ends
        let second = Arc::new(second);
        let in_flight = tokio::spawn({
            let second = second.clone();
            async move { second.wait(1).call().await }
        });
        soon("the handler runs", bench.shared.waiting(1)).await;
        bench.shut_down();
        bench.shared.open(1);
        assert_eq!(
            soon("the call in flight", in_flight)
                .await
                .unwrap()
                .unwrap(),
            1
        );
        soon("the server ends", &mut bench.served).await.unwrap();
        assert!(third.echo(1).call().await.is_err());
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
        // handlers read the same from their CallContext, shared and alone
        assert_eq!(client.call_connection().call().await.unwrap(), 0);
        assert_eq!(client.call_uid().call().await.unwrap(), 1000);
        let connections = bench.shared.connections.lock().unwrap();
        assert_eq!(connections[0].peer, peer);
        assert!(!connections[0].client.program.is_empty());
        assert_eq!(connections[0].message_len_max, MESSAGE_LEN_MAX);
    }

    /// The model of the test API, as a server decodes its own `WW_SELF_BYTES`.
    fn api_model() -> Arc<ww_self::ApiBundleOwned> {
        let bytes = ww_self::compressed::bundle_bytes(&api_impl::WW_SELF_BYTES).unwrap();
        Arc::new(ww_self::ApiBundleOwned::from_ww_bytes_owned(&bytes).unwrap())
    }

    /// What an authorization hook was asked: the peer's uid, the access, the path and the ident of the item it
    /// names.
    type Asked = Arc<Mutex<Vec<(u32, Access, Option<Vec<u32>>, Option<String>)>>>;

    /// A server whose hook lets uid 1000 do anything and others only call shared methods and read, recording what
    /// it was asked.
    fn start_authorized(path: &str, peer: Peer, asked: Asked) -> Bench {
        let hook = from_fn(move |call: &CallContext, resource: &Resource<'_>| {
            let Peer::Unix { uid, .. } = call.peer else {
                return Err(Denied);
            };
            let item = resource.item().map(|step| step.ident.to_owned());
            asked.lock().unwrap().push((
                uid.0,
                resource.access,
                resource.path.map(<[u32]>::to_vec),
                item.clone(),
            ));
            match (uid.0, resource.access, item.as_deref()) {
                (1000, _, _) | (_, Access::Read | Access::Introspect, _) => Ok(()),
                (_, Access::Call, Some("echo" | "whoami")) => Ok(()),
                _ => Err(Denied),
            }
        });
        start_bench!(
            InProcessListener::new(path, MESSAGE_LEN_MAX).with_peer(peer),
            Reach::InProcess(path.to_owned()),
            small(),
            false,
            move |server: Server<TokioClock>| server.authorize(hook, Some(api_model())),
        )
    }

    fn unix_peer(uid: u32) -> Peer {
        Peer::Unix {
            uid: Uid(uid),
            gid: Gid(100),
            pid: None,
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_hook_sees_the_peer_and_the_item_before_any_handler() {
        let asked = Asked::default();
        let bench = start_authorized(
            "host_server/authorize_owner",
            unix_peer(1000),
            asked.clone(),
        );
        let client = bench.connect().await;
        assert_eq!(client.echo(3).call().await.unwrap(), 3);
        bench.shared.open(0);
        assert_eq!(client.hold(0).call().await.unwrap(), 0);
        client.write_mode(4).write().await.unwrap();
        let asked = asked.lock().unwrap().clone();
        let at = |ident: &str| {
            asked
                .iter()
                .find(|(_, _, _, item)| item.as_deref() == Some(ident))
                .cloned()
                .unwrap_or_else(|| panic!("{ident} was asked for: {asked:?}"))
        };
        let (uid, access, path, _) = at("echo");
        assert_eq!((uid, access), (1000, Access::Call));
        assert_eq!(path.map(|p| p.len()), Some(1));
        assert_eq!(at("hold").1, Access::Call);
        assert_eq!(at("mode").1, Access::Write);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_denied_request_is_answered_denied_and_never_runs() {
        let asked = Asked::default();
        let bench = start_authorized(
            "host_server/authorize_other",
            unix_peer(1001),
            asked.clone(),
        );
        let client = bench.connect().await;
        // allowed: a shared call and a read
        assert_eq!(client.echo(3).call().await.unwrap(), 3);
        assert_eq!(client.read_mode().read().await.unwrap(), 0);
        // denied: a call that runs alone, a shared one, and a write
        let denied = |error: wire_weaver_client::Error| {
            assert_eq!(
                error.call_kind(),
                wire_weaver::call::ErrorKind::Denied,
                "{error:?}"
            );
        };
        // its handler would wait for a gate nobody opens: a run would hang the test
        denied(soon("hold", client.hold(1).call()).await.unwrap_err());
        denied(
            soon("call_uid", client.call_uid().call())
                .await
                .unwrap_err(),
        );
        denied(
            soon("write", client.write_mode(9).write())
                .await
                .unwrap_err(),
        );
        assert_eq!(bench.shared.waiting.load(Ordering::Acquire), 0);
        assert_eq!(*bench.shared.mode.lock().unwrap(), 0, "the write never ran");
        // the connection goes on
        assert_eq!(client.echo(4).call().await.unwrap(), 4);
        assert!(asked.lock().unwrap().iter().all(|(uid, ..)| *uid == 1001));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_peer_the_hook_doesnt_know_is_denied_everything() {
        let bench = start_authorized(
            "host_server/authorize_unknown",
            Peer::Unknown,
            Asked::default(),
        );
        // link setup is not a request: the client connects, every request is denied
        let client = bench.connect().await;
        let error = soon("echo", client.echo(1).call()).await.unwrap_err();
        assert_eq!(
            error.call_kind(),
            wire_weaver::call::ErrorKind::Denied,
            "{error:?}"
        );
    }

    /// A pending verdict (SRV-72) holds its request until a person answers, while the connection's other shared
    /// requests run; an approval runs it, a refusal answers `Denied`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_pending_verdict_waits_for_its_answer() {
        let (questions, mut asked) = futures_channel::mpsc::unbounded();
        let hook = from_fn(move |_: &CallContext, resource: &Resource<'_>| {
            if resource.item().map(|step| step.ident) != Some("whoami") {
                return Verdict::Allow;
            }
            let (answer, answered) = futures_channel::oneshot::channel::<bool>();
            questions.unbounded_send(answer).unwrap();
            Verdict::Pending(Pending::new(async move {
                match answered.await {
                    Ok(true) => Ok(()),
                    _ => Err(Denied),
                }
            }))
        });
        let path = "host_server/authorize_pending";
        let bench = start_bench!(
            InProcessListener::new(path, MESSAGE_LEN_MAX).with_peer(unix_peer(1000)),
            Reach::InProcess(path.to_owned()),
            small(),
            false,
            move |server: Server<TokioClock>| server.authorize(hook, Some(api_model())),
        );
        let client = bench.connect().await;
        for approve in [true, false] {
            let person = async {
                let answer = soon("the question", asked.next()).await.unwrap();
                // the question is open: other requests go on
                assert_eq!(soon("echo", client.echo(5).call()).await.unwrap(), 5);
                answer.send(approve).unwrap();
            };
            let (answered, ()) = tokio::join!(soon("whoami", client.whoami().call()), person);
            if approve {
                assert_eq!(answered.unwrap(), 0);
            } else {
                assert_eq!(
                    answered.unwrap_err().call_kind(),
                    wire_weaver::call::ErrorKind::Denied
                );
            }
        }
    }

    /// A socket in a directory of the test's own.
    #[cfg(unix)]
    fn socket_path(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ww-host-server-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("api.sock")
    }

    /// A program that answers each preamble itself, as the ww hub does, hands the connections of its API to the
    /// server, which serves them from link setup on with the peer the program passes.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn connections_the_program_opened_are_served() {
        use futures_util::io::{AsyncReadExt, AsyncWriteExt};
        use ww_byte_stream::{Answer, Preamble, Status, Target};

        let path = socket_path("opened");
        let socket = ww_local_socket::listen(&path, true).unwrap();
        let (mut sender, listener) = wire_weaver_server::opened::opened(4);
        let program = tokio::spawn(async move {
            loop {
                let (stream, peer) = socket.accept().await.unwrap();
                let (mut rx, mut tx) = stream.split();
                let preamble = Preamble::read(&mut rx).await.unwrap();
                assert!(matches!(preamble.target, Target::Device(_)));
                tx.write_all(&Answer::new(Status::Ok, "opened 0.1.0").encode())
                    .await
                    .unwrap();
                let (tx, rx, _ended) = wire_weaver_client::local_socket::device::connection_medium(
                    rx,
                    tx,
                    MESSAGE_LEN_MAX,
                );
                let peer = wire_weaver_server::local_socket::peer_of(peer);
                sender.serve(tx, rx, peer).await.unwrap();
            }
        });
        let mut bench = start_bench!(
            listener,
            Reach::LocalSocket(path.to_str().unwrap().to_owned()),
            small(),
            false,
        );
        let first = bench.connect().await;
        let second = bench.connect().await;
        assert_eq!(first.echo(3).call().await.unwrap(), 3);
        assert_eq!(second.whoami().call().await.unwrap(), 1);
        let connections = bench.shared.connections.lock().unwrap().clone();
        let Peer::Unix { pid, .. } = connections[0].peer else {
            panic!("the peer the program passed: {:?}", connections[0].peer);
        };
        assert_eq!(pid, Some(Pid(std::process::id())));
        bench.shut_down();
        soon("the server ends", &mut bench.served).await.unwrap();
        program.abort();
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn clients_on_a_local_socket_are_served_with_their_process() {
        let path = socket_path("local");
        let bench = Bench::start_local(ww_local_socket::listen(&path, true).unwrap(), &path);
        let first = bench.connect().await;
        let second = bench.connect().await;
        assert_eq!(first.whoami().call().await.unwrap(), 0);
        assert_eq!(second.whoami().call().await.unwrap(), 1);
        let uid = std::os::unix::fs::MetadataExt::uid(&std::fs::metadata(&path).unwrap());
        assert_eq!(first.call_uid().call().await.unwrap(), uid);
        let connections = bench.shared.connections.lock().unwrap().clone();
        let Peer::Unix { pid, .. } = connections[0].peer else {
            panic!(
                "a local socket's peer is a Unix process: {:?}",
                connections[0].peer
            );
        };
        assert_eq!(pid, Some(Pid(std::process::id())));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_local_client_that_sends_no_preamble_holds_up_nobody() {
        let path = socket_path("silent");
        let bench = Bench::start_local(ww_local_socket::listen(&path, true).unwrap(), &path);
        // connected, and silent: its preamble is read beside the others
        let silent: Vec<_> = futures_util::future::join_all(
            (0..wire_weaver_server::byte_stream::HANDSHAKES_MAX - 1)
                .map(|_| ww_local_socket::connect(&path)),
        )
        .await;
        let client = soon("a client next to the silent ones", bench.connect()).await;
        assert_eq!(client.echo(7).call().await.unwrap(), 7);
        drop(silent);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_answers_what_is_in_flight_then_disconnects() {
        let mut bench = Bench::start("host_server/drain", small());
        let client = Arc::new(bench.connect().await);
        let idle = bench.connect().await;
        let mut idle_events = idle.cmd.connection_events();
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
        // it is told why: the server shut down
        let reason = soon("the idle client's link ends", async {
            loop {
                if let wire_weaver_client::ConnectionEvent::Disconnected { reason, .. } =
                    idle_events.recv().await.expect("connection events")
                {
                    break reason;
                }
            }
        })
        .await;
        assert_eq!(
            reason,
            wire_weaver_client::GapReason::Disconnect(wire_weaver::DisconnectReason::Shutdown)
        );
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn clients_hear_the_reason_the_shutdown_gives() {
        let mut bench = Bench::start("host_server/reason", small());
        let client = bench.connect().await;
        let mut events = client.cmd.connection_events();
        bench.shut_down_with(DisconnectReason::Other(7));
        let reason = soon("the client's link ends", async {
            loop {
                if let wire_weaver_client::ConnectionEvent::Disconnected { reason, .. } =
                    events.recv().await.expect("connection events")
                {
                    break reason;
                }
            }
        })
        .await;
        assert_eq!(
            reason,
            wire_weaver_client::GapReason::Disconnect(DisconnectReason::Other(7))
        );
        soon("the server ends", &mut bench.served).await.unwrap();
    }

    /// News that came with the shutdown reaches the client before the disconnect: on one thread the connection
    /// sees both at once.
    #[tokio::test]
    async fn news_ready_at_shutdown_goes_out_before_the_disconnect() {
        let mut bench = Bench::start("host_server/news_at_shutdown", small());
        let client = bench.connect().await;
        let mut news = client.news().call().await.unwrap();
        bench.shared.news.send(5);
        bench.shut_down();
        assert_eq!(soon("the news", news.recv()).await.unwrap(), 5);
        soon("the server ends", &mut bench.served).await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn after_the_grace_time_a_handler_that_never_returns_is_dropped() {
        let limits = Limits {
            shutdown_grace: Duration::from_secs(3),
            ..small()
        };
        let mut bench = Bench::start("host_server/grace", limits);
        let client = Arc::new(bench.connect().await);
        // its deadline past the grace time, so that the grace ends it (SRV-47)
        let stuck = tokio::spawn({
            let client = client.clone();
            async move { client.wait(3).with_timeout(SOON).call().await }
        });
        bench.shared.waiting(1).await;
        let asked_at = tokio::time::Instant::now();
        bench.shut_down();
        (&mut bench.served).await.unwrap();
        // the grace time, then at most the linger time for the client to close
        let took = asked_at.elapsed();
        assert!(
            took >= limits.shutdown_grace
                && took <= limits.shutdown_grace + limits.disconnect_linger,
            "{took:?}"
        );
        assert!(stuck.await.unwrap().is_err());
    }

    /// A backend ends before it is dropped (SRV-74): when its client leaves, and when the server shuts down.
    #[tokio::test]
    async fn a_backend_ends_when_its_client_leaves_and_at_shutdown() {
        let mut bench = Bench::start("host_server/end", small());
        let leaves = bench.connect().await;
        let stays = bench.connect().await;
        let (left, stayed) = (
            leaves.whoami().call().await.unwrap(),
            stays.whoami().call().await.unwrap(),
        );
        drop(leaves);
        assert_eq!(
            soon(
                "the backend of the client that left",
                bench.shared.ended(left)
            )
            .await,
            [Ending::End, Ending::Drop]
        );
        assert_eq!(stays.echo(1).call().await.unwrap(), 1);
        bench.shut_down();
        soon("the server ends", &mut bench.served).await.unwrap();
        assert_eq!(
            bench.shared.ended(stayed).await,
            [Ending::End, Ending::Drop]
        );
    }

    /// An `end` that never returns is dropped after the grace time, and the server ends.
    #[tokio::test(start_paused = true)]
    async fn an_end_that_never_returns_is_dropped_after_the_grace_time() {
        let limits = Limits {
            shutdown_grace: Duration::from_secs(3),
            ..small()
        };
        let mut bench = Bench::start("host_server/end_hangs", limits);
        bench.shared.end_hangs.store(true, Ordering::Release);
        let client = bench.connect().await;
        let id = client.whoami().call().await.unwrap();
        let left_at = tokio::time::Instant::now();
        drop(client);
        assert_eq!(bench.shared.ended(id).await, [Ending::Drop]);
        assert!(
            left_at.elapsed() >= limits.shutdown_grace,
            "{:?}",
            left_at.elapsed()
        );
        bench.shut_down();
        (&mut bench.served).await.unwrap();
    }

    mod byte_streams;
    mod udp;
}

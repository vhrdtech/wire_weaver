//! The mount forwarder (SRV-78) between real servers: a hub process serves `mount_backend_api` on a local socket,
//! the TV of `mounts_api` mounts it at its hub slot and forwards to it with `wire_weaver_server::mount`, a client
//! reaches the hub through the TV.

use std::sync::{Arc, Mutex};

use wire_weaver::prelude::*;
use wire_weaver::ww_api::mount::{MountNameOwned, MountPresenceOwned};
use wire_weaver::{EventWriter, MessageSink};
use wire_weaver_client::in_process::TokioClock;
use wire_weaver_server::Connection;
use wire_weaver_server::mount::{Forwarder, LocalSocket, MountEvent, Mounts};
use ww_client_server::ResourceIndex;
use ww_client_server::mount::{MountAnswer, MountRequest};
use ww_device::Instant;

/// The entries a host server calls, forwarded to the generated ones of `$server`, whose API's id is `$gid`.
macro_rules! backend_entries {
    ($server:ty, $gid:expr) => {
        impl wire_weaver::WireWeaverAsyncApiBackend for $server {
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
                $gid
            }
        }

        impl wire_weaver::WireWeaverSharedApiBackend for $server {
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
                <$server>::is_shared(data)
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
    };
}

/// The hub process: adds, hangs, toggles its lamp.
pub mod hub {
    use super::*;
    use wire_weaver::ReplyTo;

    /// Requests of `hang`, never answered, and the seqs the hub's clients cancelled.
    #[derive(Default)]
    pub struct Hung(pub Mutex<Vec<ReplyTo<()>>>, pub Mutex<Vec<u32>>);

    pub struct Hub {
        pub hung: Arc<Hung>,
        pub lamp_on: bool,
    }

    impl wire_weaver_server::Backend for Hub {
        fn cancel(&mut self, seq: u32) {
            self.hung.1.lock().unwrap().push(seq);
        }
    }

    impl Hub {
        async fn add(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            a: u32,
            b: u32,
        ) -> RpcResult<u32> {
            Ready(a + b)
        }

        async fn hang(&mut self, cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
            if let Some(reply_to) = cx.reply_to() {
                self.hung.0.lock().unwrap().push(reply_to);
            }
            Deferred
        }

        async fn lamp_toggle(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<bool> {
            self.lamp_on = !self.lamp_on;
            Ready(self.lamp_on)
        }
    }

    backend_entries!(Hub, mount_backend_api::HUB_FULL_GID);

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            mount_backend_api :: Hub for super::Hub,
            server = true, no_alloc = false, use_async = true,
            method_model = "hang=deferred, _=immediate",
            property_model = "_=get_set",
            introspect = "no_docs",
            debug_to_file = "../../target/tests_mounts_hub.rs"
        );
    }
}

/// The TV: its hub slot forwards to whatever the mount table has there; `ext` has no slots.
pub mod tv {
    use super::*;

    pub struct Tv {
        pub mounts: Mounts<LocalSocket>,
        pub forwarder: Forwarder<LocalSocket, TokioClock>,
        pub valid: Vec<UNib32>,
        pub observe: api_impl::ObserveState,
        /// What `poll` got from the mount table, for the tests.
        pub seen: Arc<Mutex<Vec<MountEvent>>>,
    }

    impl Tv {
        /// The backend of `connection`; `clock` is the server's, which the forwarder's timers must share.
        pub fn new(
            mounts: &Mounts<LocalSocket>,
            connection: &Connection,
            clock: TokioClock,
            seen: Arc<Mutex<Vec<MountEvent>>>,
        ) -> Self {
            Tv {
                mounts: mounts.clone(),
                forwarder: Forwarder::new(mounts, connection, clock),
                valid: vec![],
                observe: api_impl::ObserveState::new(Default::default()),
                seen,
            }
        }

        async fn get_hub_presence(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
        ) -> GetResult<MountPresenceOwned, CommonError<'_>> {
            Value(self.mounts.presence(&super::hub_api_path()))
        }

        async fn hub_request(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            request: MountRequest<'_, '_>,
        ) -> MountAnswer {
            self.forwarder.forward(&request)
        }

        fn valid_indices_root_ext(&mut self) -> ValidIndices<'_> {
            ValidIndices::List(RefVec::Slice { slice: &self.valid })
        }

        async fn get_ext_presence(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            _index_chain: [ResourceIndex; 1],
        ) -> GetResult<MountPresenceOwned, CommonError<'_>> {
            Value(MountPresenceOwned::Absent)
        }

        async fn ext_request(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            _index_chain: [ResourceIndex; 1],
            request: MountRequest<'_, '_>,
        ) -> MountAnswer {
            self.forwarder.forward(&request)
        }

        async fn ext_index_of(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            _key: MountNameOwned,
        ) -> RpcResult<Option<UNib32>> {
            Ready(None)
        }

        async fn get_ext_key(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            _index_chain: [ResourceIndex; 1],
        ) -> GetResult<MountNameOwned, CommonError<'_>> {
            Value(MountNameOwned(
                MaxLen::new(String::new()).expect("empty fits"),
            ))
        }

        async fn mute(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
            Ready(())
        }

        fn observe_state(&mut self) -> &mut api_impl::ObserveState {
            &mut self.observe
        }
    }

    backend_entries!(Tv, mounts_api::TV_FULL_GID);

    impl wire_weaver_server::Backend for Tv {
        async fn wait(&mut self) {
            self.forwarder.wait().await;
        }

        fn next_wake(&mut self, _now: Instant) -> Option<Instant> {
            self.forwarder.next_wake()
        }

        async fn poll(
            &mut self,
            out: &mut EventWriter<'_, impl MessageSink>,
            _medium: (),
            _now: Instant,
        ) {
            let changes = self.forwarder.poll(out).await;
            for change in changes {
                if let MountEvent::Presence { .. } = change {
                    self.mark_changed_hub_presence();
                }
                self.seen.lock().unwrap().push(change);
            }
        }

        fn cancel(&mut self, seq: u32) {
            self.forwarder.cancel(seq);
        }

        async fn end(&mut self) {
            self.forwarder.end().await;
        }
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            mounts_api :: Tv for super::Tv,
            server = true, no_alloc = false, use_async = true,
            method_model = "_=immediate",
            property_model = "_=get_set",
            introspect = "no_docs",
        );
    }
}

/// Path of the hub slot's `api` in the TV: `hub` is id 0, a slot's `api` id 1.
pub fn hub_api_path() -> Vec<ResourceIndex> {
    vec![ResourceIndex::new(UNib32(0)), ResourceIndex::new(UNib32(1))]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use tokio::sync::oneshot;
    use tokio::task::JoinHandle;
    use wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned;
    use wire_weaver::ww_version::FullVersionOwned;
    use wire_weaver_client::internal::Command;
    use wire_weaver_client::{ClientConfig, Commander, DynClient, Error};
    use wire_weaver_server::Server;
    use wire_weaver_server::byte_stream::{PreambleFraming, listen};
    use wire_weaver_server::in_process::InProcessListener;
    use wire_weaver_server::local_socket::LocalSocketIncoming;
    use wire_weaver_server::mount::MountOptions;
    use ww_client_server::{
        ErrorKindOwned, ErrorOwned, PathKindOwned, RequestKindOwned, RequestOwned,
    };
    use ww_self::ApiBundleOwned;

    const MESSAGE_LEN_MAX: usize = 4096;
    /// How long a test waits for what must happen.
    const SOON: Duration = Duration::from_secs(5);

    // ids: the TV's `hub` 0 (`api` 1 below it), `mute` 3; the hub's `add` 0, `hang` 1, `lamp` 2; the lamp's
    // `toggle` 0
    const HUB_API: [u32; 2] = [0, 1];
    const MUTE: u32 = 3;
    const ADD: u32 = 0;
    const HANG: u32 = 1;
    const LAMP: u32 = 2;
    const TOGGLE: u32 = 0;

    #[derive_shrink_wrap(owned)]
    struct AddArgs {
        a: u32,
        b: u32,
    }

    /// The hub process, serving on a local socket until told to stop.
    struct HubProcess {
        stop: Option<oneshot::Sender<()>>,
        served: JoinHandle<()>,
        hung: Arc<hub::Hung>,
    }

    impl HubProcess {
        fn start(path: &Path) -> Self {
            let socket = ww_local_socket::listen(path, true).expect("the hub listens");
            let listener = listen(
                LocalSocketIncoming::new(socket),
                PreambleFraming::new("hub 0.1.0", MESSAGE_LEN_MAX),
                TokioClock::new(),
            );
            let hung = Arc::new(hub::Hung::default());
            let for_backends = hung.clone();
            let (stop, stopped) = oneshot::channel::<()>();
            let link = ww_device::LinkConfig::new(
                mount_backend_api::HUB_FULL_GID,
                mount_backend_api::HUB_WIRE_VERSION,
                hub::api_impl::api_hash(),
                hub::api_impl::image_facts(),
                ww_client_server::COMPACT_VERSION,
            );
            let served = tokio::spawn(async move {
                let make = move |_: &Connection| hub::Hub {
                    hung: for_backends.clone(),
                    lamp_on: false,
                };
                let shutdown = async {
                    let _ = stopped.await;
                };
                Server::new(TokioClock::new())
                    .serve(link, listener, make, shutdown)
                    .await
                    .expect("the hub serves");
            });
            HubProcess {
                stop: Some(stop),
                served,
                hung,
            }
        }

        async fn stop(mut self) {
            let _ = self.stop.take().expect("stopped once").send(());
            soon("the hub stops", self.served).await.unwrap();
        }
    }

    /// The TV, in process, with its mount table.
    struct TvProcess {
        mounts: Mounts<LocalSocket>,
        path: String,
        _served: JoinHandle<()>,
        _run: JoinHandle<()>,
    }

    impl TvProcess {
        fn start(path: &str) -> Self {
            let clock = TokioClock::new();
            let own = ww_self::compressed::bundle_bytes(&tv::api_impl::WW_SELF_BYTES).unwrap();
            let own = Arc::new(ApiBundleOwned::from_ww_bytes_owned(&own).unwrap());
            let mounts = Mounts::<LocalSocket>::new(own);
            let link = ww_device::LinkConfig::new(
                mounts_api::TV_FULL_GID,
                mounts_api::TV_WIRE_VERSION,
                tv::api_impl::api_hash(),
                tv::api_impl::image_facts(),
                ww_client_server::COMPACT_VERSION,
            );
            let listener = InProcessListener::new(path, MESSAGE_LEN_MAX);
            let for_backends = mounts.clone();
            let seen = Arc::new(Mutex::new(Vec::new()));
            let server = Server::new(clock).mounts(mounts.routes());
            let served = tokio::spawn(async move {
                let make = move |connection: &Connection| {
                    tv::Tv::new(&for_backends, connection, clock, seen.clone())
                };
                server
                    .serve(link, listener, make, std::future::pending::<()>())
                    .await
                    .expect("the TV serves");
            });
            let table = mounts.clone();
            let run = tokio::spawn(async move { table.run(clock).await });
            TvProcess {
                mounts,
                path: path.to_owned(),
                _served: served,
                _run: run,
            }
        }

        fn mount_hub(&self, socket: &Path) {
            let options = MountOptions {
                retry_after: Duration::from_millis(20),
                ..MountOptions::default()
            };
            self.mounts
                .mount(&hub_api_path(), LocalSocket::new(socket), options);
        }

        /// Waits until the hub slot's presence is up (`true`) or absent.
        async fn hub_up(&self, up: bool) {
            soon("the hub's presence", async {
                while matches!(
                    self.mounts.presence(&hub_api_path()),
                    MountPresenceOwned::Up { .. }
                ) != up
                {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await;
        }

        async fn client(&self) -> DynClient {
            DynClient::from_config(
                ClientConfig::new()
                    .in_process_path(self.path.clone())
                    .resolver(tests_common::resolver()),
            )
            .connect()
            .await
            .expect("connect")
        }
    }

    async fn soon<T>(what: &str, f: impl std::future::Future<Output = T>) -> T {
        tokio::time::timeout(SOON, f)
            .await
            .unwrap_or_else(|_| panic!("{what}: still waiting"))
    }

    fn socket_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ww-mounts-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("hub.sock")
    }

    fn absolute(path: &[u32]) -> PathKindOwned {
        PathKindOwned::Absolute {
            path: path
                .iter()
                .map(|i| ResourceIndex::new(UNib32(*i)))
                .collect(),
        }
    }

    fn under_hub(path: &[u32]) -> PathKindOwned {
        let full: Vec<u32> = HUB_API.iter().chain(path).copied().collect();
        absolute(&full)
    }

    fn call(args: Vec<u8>) -> RequestKindOwned {
        RequestKindOwned::Call {
            args: TailBytesOwned(args),
        }
    }

    fn add_args(a: u32, b: u32) -> Vec<u8> {
        AddArgs { a, b }.to_ww_bytes_owned().unwrap()
    }

    /// Send a request as is and wait for the answer: the value bytes, or the error.
    async fn raw(
        cmd: &Commander,
        path_kind: PathKindOwned,
        kind: RequestKindOwned,
    ) -> Result<Vec<u8>, Error> {
        let bytes = RequestOwned {
            seq: UVlq32Backfill(0),
            priority: ww_client_server::Priority::NORMAL.to_wire(),
            context: None,
            path_kind,
            kind,
        }
        .to_ww_bytes_owned()
        .unwrap();
        let (tx, rx) = oneshot::channel();
        cmd.send(Command::SendMessage {
            bytes,
            done_tx: Some((tx, SOON)),
            trace: None,
        })
        .await?;
        rx.await.expect("event loop answers")
    }

    fn assert_refused(answer: Result<Vec<u8>, Error>, kind: ErrorKindOwned) {
        match answer {
            Err(Error::RemoteError(ErrorOwned { kind: got, .. }))
                if std::mem::discriminant(&got) == std::mem::discriminant(&kind) => {}
            other => panic!("expected {kind:?}, got {other:?}"),
        }
    }

    async fn add(cmd: &Commander, a: u32, b: u32) -> Result<u32, Error> {
        let value = raw(cmd, under_hub(&[ADD]), call(add_args(a, b))).await?;
        Ok(u32::from_ww_bytes(&value).unwrap())
    }

    /// Calls, a nested trait by its absolute path and by a global one, the TV's own items: all through one link.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn requests_reach_the_backend() {
        let socket = socket_path("reach");
        let hub = HubProcess::start(&socket);
        let tv = TvProcess::start("mounts/forwarder/reach");
        tv.mount_hub(&socket);
        tv.hub_up(true).await;
        let client = tv.client().await;
        let cmd = client.cmd();
        assert_eq!(add(cmd, 2, 3).await.unwrap(), 5);
        let toggled = raw(cmd, under_hub(&[LAMP, TOGGLE]), call(vec![]))
            .await
            .unwrap();
        assert!(bool::from_ww_bytes(&toggled).unwrap());
        // the TV has no `Lamp`: its global path reaches into the hub
        let (major, minor, patch) = mount_backend_api_version();
        let global = PathKindOwned::GlobalFull {
            gid: FullVersionOwned::new(
                "mount_backend_api".to_owned(),
                wire_weaver::ww_version::VersionOwned::new(major, minor, patch),
            ),
            trait_name: "Lamp".to_owned(),
            path_from_trait: vec![ResourceIndex::new(UNib32(TOGGLE))],
        };
        let toggled = raw(cmd, global, call(vec![])).await.unwrap();
        assert!(!bool::from_ww_bytes(&toggled).unwrap());
        // the TV's own items stay its own
        assert_eq!(
            raw(cmd, absolute(&[MUTE]), call(vec![])).await.unwrap(),
            Vec::<u8>::new()
        );
        let missing = raw(cmd, under_hub(&[9]), call(vec![])).await;
        assert_refused(missing, ErrorKindOwned::BadPath);
        hub.stop().await;
        std::fs::remove_dir_all(socket.parent().unwrap()).unwrap();
    }

    fn mount_backend_api_version() -> (u32, u32, u32) {
        let v = &mount_backend_api::HUB_FULL_GID.version;
        (v.major.0, v.minor.0, v.patch.0)
    }

    /// A backend that goes away fails what it didn't answer and turns the slot absent; mounted again, it is used
    /// again.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn backend_goes_and_comes_back() {
        let socket = socket_path("gone");
        let hub = HubProcess::start(&socket);
        let tv = TvProcess::start("mounts/forwarder/gone");
        tv.mount_hub(&socket);
        tv.hub_up(true).await;
        let client = tv.client().await;
        let cmd = client.cmd().clone();
        assert_eq!(add(&cmd, 1, 1).await.unwrap(), 2);
        let hanging = {
            let cmd = cmd.clone();
            tokio::spawn(async move { raw(&cmd, under_hub(&[HANG]), call(vec![])).await })
        };
        soon("the hub holds the request", async {
            while hub.hung.0.lock().unwrap().is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        hub.stop().await;
        let answer = soon("the hanging request", hanging).await.unwrap();
        assert_refused(answer, ErrorKindOwned::Unavailable);
        tv.hub_up(false).await;
        assert_refused(
            add(&cmd, 1, 2).await.map(|_| vec![]),
            ErrorKindOwned::Unavailable,
        );
        let hub = HubProcess::start(&socket);
        tv.hub_up(true).await;
        assert_eq!(add(&cmd, 3, 4).await.unwrap(), 7);
        hub.stop().await;
        std::fs::remove_dir_all(socket.parent().unwrap()).unwrap();
    }

    /// A client that stops waiting cancels its request: the TV doesn't hold it, so the cancel goes on to the hub.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancel_reaches_the_backend() {
        let socket = socket_path("cancel");
        let hub = HubProcess::start(&socket);
        let tv = TvProcess::start("mounts/forwarder/cancel");
        tv.mount_hub(&socket);
        tv.hub_up(true).await;
        let client = tv.client().await;
        let cmd = client.cmd().clone();
        let hanging = {
            let cmd = cmd.clone();
            tokio::spawn(async move { raw(&cmd, under_hub(&[HANG]), call(vec![])).await })
        };
        soon("the hub holds the request", async {
            while hub.hung.0.lock().unwrap().is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        let seq = hub.hung.0.lock().unwrap()[0].seq;
        hanging.abort();
        // what a generated call's dropped answer sends: the event loop looks for callers gone
        let _ = hanging.await;
        cmd.send(Command::CallerGone).await.unwrap();
        soon("the hub sees the cancel", async {
            while !hub.hung.1.lock().unwrap().contains(&seq) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        hub.stop().await;
        std::fs::remove_dir_all(socket.parent().unwrap()).unwrap();
    }

    /// Nothing mounted: refused at once.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn nothing_mounted() {
        let tv = TvProcess::start("mounts/forwarder/nothing");
        let client = tv.client().await;
        assert_refused(
            add(client.cmd(), 1, 2).await.map(|_| vec![]),
            ErrorKindOwned::Unavailable,
        );
    }
}

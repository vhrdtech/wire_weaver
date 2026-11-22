//! Reconnects onto another revision of the device's API on the same wire line (CLT-81), end to end: a generated
//! client of `reconnect_revision_api` (w1.0) against generated servers of it, of `reconnect_revision_added_api` (w1.1,
//! a method added) and of `reconnect_revision_changed_api` (w1.1, the item type of `readings` changed), on the
//! simulated WebSocket (TR-22, `tests_common::start_device_async_ws`) and tokio's paused clock. Each reconnect learns
//! the device's API again: on the compatible revision every stream and observer goes on, on the changed one
//! `readings` ends and its calls are refused while the rest goes on. Through a keep-trying connection,
//! `Commander::reconnect` and `Commander::reconnect_as`; a clone made before sees the new device.

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::sync::broadcast;
    use wire_weaver::prelude::*;
    use wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned;
    use wire_weaver::ww_version::{FullVersion, Version, WireVersion};
    use wire_weaver_client::observe::{ObserveEvent, Observer, periodic as every};
    use wire_weaver_client::{ConnectionEvent, Error, KeepTrying, PositionedStream, StreamEvent};
    use ww_si::Fixed;
    use ww_si::prefix::One;
    use ww_si::units::Item;
    use ww_stream::tx::Counted;

    type Ticks = Fixed<Item, One, u32>;

    /// The server of one revision: `$api`'s generated code on a struct named `$server`, whose `readings` items are
    /// `Fixed<Item, One, $reading>`; `$extra` are the revision's own handlers.
    macro_rules! server {
        ($module:ident, $api:ident, $reading:ty, { $($extra:tt)* }) => {
            mod $module {
                use super::*;

                pub struct Server {
                    pub ticks: u32,
                    pub observe: api_impl::ObserveState,
                    pub kept_stream: Counted,
                    pub readings_stream: Counted,
                    pub stream_sessions: u32,
                    pub buf: Vec<u8>,
                }

                impl Server {
                    /// A device whose streams have a session already: firmware that kept them over its update, as a
                    /// device that keeps them in retained RAM does, so that a host's counted streams can resume.
                    pub fn new(sessions: u32) -> Self {
                        let session = |s| {
                            let mut counted = Counted::new();
                            if s > 0 {
                                counted.open(s);
                                counted.detach();
                            }
                            counted
                        };
                        Server {
                            ticks: 0,
                            observe: api_impl::ObserveState::new(Default::default()),
                            kept_stream: session(sessions),
                            readings_stream: session(sessions),
                            stream_sessions: sessions,
                            buf: vec![0; 256],
                        }
                    }

                    async fn get_ticks(
                        &mut self,
                        _cx: &mut Context<'_, impl EventOut>,
                    ) -> GetResult<Ticks, CommonError<'_>> {
                        GetResult::Value(Fixed::new(self.ticks))
                    }

                    async fn kept(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
                        RpcResult::Ready(())
                    }

                    async fn close_kept(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

                    async fn stream_kept(&mut self) -> &mut Counted {
                        &mut self.kept_stream
                    }

                    async fn readings(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
                        RpcResult::Ready(())
                    }

                    async fn close_readings(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

                    async fn stream_readings(&mut self) -> &mut Counted {
                        &mut self.readings_stream
                    }

                    async fn emit(&mut self, cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
                        let ser = api_impl::stream_data_ser();
                        // a stream no host has open drops its event
                        _ = ser.kept_send(&mut self.kept_stream, &[1, 2, 3], 3, None, cx).await;
                        let reading: Fixed<Item, One, $reading> = Fixed::new(7);
                        _ = ser
                            .readings_send(&mut self.readings_stream, &reading, 1, None, cx)
                            .await;
                        RpcResult::Ready(())
                    }

                    $($extra)*

                    fn new_stream_session(&mut self) -> u32 {
                        self.stream_sessions = self.stream_sessions.wrapping_add(1);
                        self.stream_sessions
                    }

                    fn observe_state(&mut self) -> &mut api_impl::ObserveState {
                        &mut self.observe
                    }
                }

                pub mod api_impl {
                    wire_weaver::ww_codegen!(
                        $api :: Revision for super::Server,
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
                        self.process_request_bytes(bytes, scratch, out, medium).await
                    }

                    async fn poll(&mut self, out: &mut impl EventOut, now_ms: u64) {
                        self.ticks += 1;
                        let mut buf = core::mem::take(&mut self.buf);
                        _ = self.poll_properties(out, (), now_ms * 1000, &mut buf).await;
                        self.buf = buf;
                    }
                }
            }
        };
    }

    server!(base, reconnect_revision_api, u8, {});
    server!(added, reconnect_revision_added_api, u8, {
        async fn added(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
            RpcResult::Ready(())
        }
    });
    server!(changed, reconnect_revision_changed_api, u32, {});

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
                reconnect_revision_api :: Revision for super::Client,
                client = "std_client",
            );
        }
    }

    use client::Client;
    use client::api_client::reconnect_revision_api_revision::RevisionUpdate;

    const MS: Duration = Duration::from_millis(1);

    #[derive(Clone, Copy, Debug)]
    enum Rev {
        /// w1.0, the client's own API
        Base,
        /// w1.1, a method added
        Added,
        /// w1.1, the item type of `readings` changed
        Changed,
    }

    /// A device of `rev` at `path`, reporting itself as `reconnect_revision_api` (0.1.0 at w1.0, 0.1.1 at w1.1), its
    /// streams in a session from before unless it is the first device.
    fn start(path: &str, rev: Rev) -> tests_common::TestDevice {
        let _ = tracing_subscriber::fmt::try_init();
        let w11 = WireVersion::new(1, 1);
        let v = |patch| FullVersion::new("reconnect_revision_api", Version::new(0, 1, patch));
        match rev {
            Rev::Base => tests_common::start_device_async_ws(
                path,
                base::Server::new(0),
                v(0),
                reconnect_revision_api::REVISION_WIRE_VERSION,
                tests_common::api!(base::api_impl),
            ),
            Rev::Added => tests_common::start_device_async_ws(
                path,
                added::Server::new(1),
                v(1),
                w11,
                tests_common::api!(added::api_impl),
            ),
            Rev::Changed => tests_common::start_device_async_ws(
                path,
                changed::Server::new(1),
                v(1),
                w11,
                tests_common::api!(changed::api_impl),
            ),
        }
    }

    /// What a host has open on the device, to see after a reconnect.
    struct Open {
        kept: PositionedStream<TailBytesOwned>,
        readings: PositionedStream<Fixed<Item, One, u8>>,
        ticks: Observer<RevisionUpdate, Ticks>,
    }

    impl Open {
        async fn new(client: &Client) -> Self {
            let mut open = Open {
                kept: client.kept().call().await.expect("open kept"),
                readings: client.readings().call().await.expect("open readings"),
                ticks: client
                    .observe_ticks(every(10 * MS))
                    .subscribe()
                    .await
                    .expect("observe"),
            };
            open.delivers(client).await;
            open
        }

        /// Every stream and the observer deliver.
        async fn delivers(&mut self, client: &Client) {
            client.emit().call().await.expect("emit");
            next_item(&mut self.kept).await.expect("kept goes on");
            next_item(&mut self.readings)
                .await
                .expect("readings goes on");
            self.ticks_count_up().await;
        }

        async fn ticks_count_up(&mut self) {
            let first = next_tick(&mut self.ticks).await;
            assert!(next_tick(&mut self.ticks).await > first, "counts up");
        }

        /// `readings` ended, the rest delivers.
        async fn readings_ended(&mut self, client: &Client) {
            client.emit().call().await.expect("emit");
            next_item(&mut self.kept).await.expect("kept goes on");
            assert!(
                next_item(&mut self.readings).await.is_err(),
                "readings ended"
            );
            self.ticks_count_up().await;
        }
    }

    /// The next item of `stream`, past a `Resumed` and gaps; `Err` once the stream ended.
    async fn next_item<T: DeserializeShrinkWrapOwned + std::fmt::Debug>(
        stream: &mut PositionedStream<T>,
    ) -> Result<(), wire_weaver_client::StreamError> {
        loop {
            match stream.recv_event().await? {
                StreamEvent::Item { .. } => return Ok(()),
                StreamEvent::Closed => return Err(wire_weaver_client::StreamError::Closed),
                StreamEvent::Resumed { .. }
                | StreamEvent::Gap { .. }
                | StreamEvent::Disconnected => {}
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    /// The next update of `ticks`, past a `Disconnected` and a `Resumed`.
    async fn next_tick(observer: &mut Observer<RevisionUpdate, Ticks>) -> u32 {
        loop {
            match observer.recv_event().await.expect("observer goes on") {
                ObserveEvent::Update(u) => return u.value.value,
                ObserveEvent::Disconnected | ObserveEvent::Resumed => {}
                other => panic!("unexpected {other:?}"),
            }
        }
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

    async fn connect(path: &str) -> Client {
        Client::config(|c| {
            c.in_process_path(path.to_owned())
                .resolver(tests_common::resolver())
        })
        .connect()
        .await
        .expect("connect")
    }

    /// A keep-trying client of the device at `path`, connected to it.
    async fn keep_trying(path: &str) -> (Client, broadcast::Receiver<ConnectionEvent>) {
        let client = Client::config(|c| {
            c.in_process_path(path.to_owned())
                .resolver(tests_common::resolver())
                .keep_trying(KeepTrying::default())
        })
        .connect_keep_trying()
        .expect("the handle is made at once");
        let mut events = client.cmd.connection_events();
        match next_outcome(&mut events).await {
            ConnectionEvent::Connected { .. } => {}
            event => panic!("expected Connected, got {event:?}"),
        }
        (client, events)
    }

    /// The device at `path` goes away and comes back as `rev`; the keep-trying client reconnects.
    async fn replace_kept_trying(
        device: tests_common::TestDevice,
        path: &str,
        rev: Rev,
        events: &mut broadcast::Receiver<ConnectionEvent>,
    ) -> tests_common::TestDevice {
        drop(device);
        match next_outcome(events).await {
            ConnectionEvent::Disconnected {
                reconnecting: true, ..
            } => {}
            event => panic!("expected Disconnected, got {event:?}"),
        }
        let device = start(path, rev);
        match next_outcome(events).await {
            ConnectionEvent::Reconnected { .. } => {}
            event => panic!("expected Reconnected, got {event:?}"),
        }
        device
    }

    /// The host disconnects keeping its streams, the device at `path` is replaced by one of `rev`.
    async fn replace(
        client: &Client,
        device: tests_common::TestDevice,
        path: &str,
        rev: Rev,
    ) -> tests_common::TestDevice {
        client
            .cmd
            .disconnect()
            .keep_streams()
            .await
            .expect("disconnect");
        drop(device);
        start(path, rev)
    }

    fn version(client: &wire_weaver_client::Commander) -> (u32, Option<WireVersion>) {
        let info = client.device_api_info();
        (info.user_api_version.version.patch.0, info.user_api_wire)
    }

    async fn run(test: impl Future<Output = ()>) {
        tokio::task::LocalSet::new().run_until(test).await
    }

    /// A keep-trying connection reconnects onto a revision with a method added: the device's API is learned again,
    /// every stream and observer goes on, calls go through, and a clone made before sees the new device.
    #[tokio::test(start_paused = true)]
    async fn keep_trying_onto_a_compatible_revision() {
        run(async {
            const PATH: &str = "reconnect_revision/keep_trying_compatible";
            let device = start(PATH, Rev::Base);
            let (client, mut events) = keep_trying(PATH).await;
            let early = client.cmd.clone();
            let mut open = Open::new(&client).await;
            let before = client.cmd.device_api_info().user_api_hash.no_docs.hash;

            let _device = replace_kept_trying(device, PATH, Rev::Added, &mut events).await;
            open.delivers(&client).await;
            assert_eq!(version(&early), (1, Some(WireVersion::new(1, 1))));
            assert_ne!(early.device_api_info().user_api_hash.no_docs.hash, before);
            assert!(early.device_introspect().is_some(), "learned again");
            client.read_ticks().read().await.expect("calls go through");
        })
        .await
    }

    /// A keep-trying connection reconnects onto a revision whose `readings` item type changed: that stream ends and
    /// opening it is refused before anything is sent; the other stream, the observer and the other calls go on.
    #[tokio::test(start_paused = true)]
    async fn keep_trying_onto_an_incompatible_stream() {
        run(async {
            const PATH: &str = "reconnect_revision/keep_trying_incompatible";
            let device = start(PATH, Rev::Base);
            let (client, mut events) = keep_trying(PATH).await;
            let mut open = Open::new(&client).await;

            let _device = replace_kept_trying(device, PATH, Rev::Changed, &mut events).await;
            open.readings_ended(&client).await;
            assert!(matches!(
                client.readings().call().await,
                Err(Error::IncompatibleResource { .. })
            ));
            client.read_ticks().read().await.expect("calls go through");
        })
        .await
    }

    /// `Commander::reconnect` onto a compatible revision resumes everything, onto the changed one ends `readings` only.
    #[tokio::test(start_paused = true)]
    async fn reconnect_onto_revisions() {
        run(async {
            const PATH: &str = "reconnect_revision/reconnect";
            let device = start(PATH, Rev::Base);
            let client = connect(PATH).await;
            let early = client.cmd.clone();
            let mut open = Open::new(&client).await;

            let device = replace(&client, device, PATH, Rev::Added).await;
            let resumed = client.cmd.reconnect().await.expect("a compatible revision");
            assert_eq!(resumed, 3, "two streams and the observed level");
            assert_eq!(version(&early), (1, Some(WireVersion::new(1, 1))));
            open.delivers(&client).await;

            let _device = replace(&client, device, PATH, Rev::Changed).await;
            let resumed = client.cmd.reconnect().await.expect("the same line");
            assert_eq!(resumed, 2, "kept and the observed level");
            open.readings_ended(&client).await;
            assert!(matches!(
                client.readings().call().await,
                Err(Error::IncompatibleResource { .. })
            ));
        })
        .await
    }

    /// `Commander::reconnect_as` on the same line, onto another revision: the same as `reconnect`, and the client it
    /// returns works.
    #[tokio::test(start_paused = true)]
    async fn reconnect_as_onto_revisions() {
        run(async {
            const PATH: &str = "reconnect_revision/reconnect_as";
            let device = start(PATH, Rev::Base);
            let client = connect(PATH).await;
            let mut open = Open::new(&client).await;

            let device = replace(&client, device, PATH, Rev::Added).await;
            let again: Client = client
                .cmd
                .reconnect_as()
                .await
                .expect("a compatible revision");
            assert_eq!(version(&again.cmd), (1, Some(WireVersion::new(1, 1))));
            open.delivers(&again).await;

            let _device = replace(&again, device, PATH, Rev::Changed).await;
            let again: Client = again.cmd.reconnect_as().await.expect("the same line");
            open.readings_ended(&again).await;
            assert!(matches!(
                again.readings().call().await,
                Err(Error::IncompatibleResource { .. })
            ));
            again.read_ticks().read().await.expect("calls go through");
        })
        .await
    }
}

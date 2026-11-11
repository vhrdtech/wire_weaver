//! Observed properties under resource arrays (API-63), end to end: the generated server with an element table per
//! observed element, polled every millisecond with a clock that moves one millisecond per poll, and the generated
//! client through the real host event loop (`tests_common`). Each element is observed by its own path and gets its
//! own updates; a removed element ends its observers.

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::time::Duration;
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver_client::observe::{ObserveEvent, on_change};
    use wire_weaver_client::{Error, StreamError};

    mod server {
        use super::*;
        use tests_common::TestProcessEventsAsync;

        pub struct Server {
            pub browser_state: [u8; 3],
            pub browsers_zoom: [u8; 3],
            /// Players by id, with their titles
            pub players: Vec<(u32, String)>,
            /// Their ids, for `valid_indices`
            pub player_ids: Vec<UNib32>,
            pub observe: api_impl::ObserveState,
            /// Microseconds, one millisecond more per poll
            pub now_us: u64,
            pub buf: Vec<u8>,
        }

        impl Server {
            fn observe_state(&mut self) -> &mut api_impl::ObserveState {
                &mut self.observe
            }

            fn valid_indices_root_browsers(&mut self) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..3)
            }

            async fn get_volume(&mut self, _cx: &mut Context<'_, impl EventOut>) -> GetResult<u8, ()> {
                GetResult::Value(7)
            }

            async fn set_browser_state(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                slot: u32,
                state: u8,
            ) -> RpcResult<()> {
                self.browser_state[slot as usize] = state;
                self.mark_changed_browsers_state([UNib32(slot)]);
                RpcResult::Ready(())
            }

            async fn set_title(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                player: u32,
                title: String,
            ) -> RpcResult<()> {
                if let Some((_, t)) = self.players.iter_mut().find(|(id, _)| *id == player) {
                    *t = title;
                    self.mark_changed_media_players_playback_title([UNib32(player)]);
                }
                RpcResult::Ready(())
            }

            async fn add_player(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                player: u32,
            ) -> RpcResult<()> {
                self.players.push((player, format!("new {player}")));
                self.player_ids.push(UNib32(player));
                RpcResult::Ready(())
            }

            async fn remove_player(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                player: u32,
            ) -> RpcResult<()> {
                self.players.retain(|(id, _)| *id != player);
                self.player_ids.retain(|id| id.0 != player);
                self.element_removed_media_players([UNib32(player)]);
                RpcResult::Ready(())
            }

            async fn barrier(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
                RpcResult::Ready(())
            }

            async fn get_browsers_state(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                index_chain: [UNib32; 1],
            ) -> GetResult<u8, ()> {
                GetResult::Value(self.browser_state[index_chain[0].0 as usize])
            }

            async fn changed_browsers_zoom(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                _index_chain: [UNib32; 1],
            ) {
            }

            async fn get_browsers_playback_title(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                index_chain: [UNib32; 1],
            ) -> GetResult<String, ()> {
                GetResult::Value(format!("browser {}", index_chain[0].0))
            }

            async fn browsers_playback_play(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                _index_chain: [UNib32; 1],
            ) -> RpcResult<()> {
                RpcResult::Ready(())
            }

            fn valid_indices_root_media_players(&mut self) -> ValidIndices<'_> {
                ValidIndices::List(RefVec::Slice {
                    slice: &self.player_ids,
                })
            }

            async fn get_media_players_source(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                _index_chain: [UNib32; 1],
            ) -> GetResult<u8, ()> {
                GetResult::Value(0)
            }

            async fn get_media_players_playback_title(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                index_chain: [UNib32; 1],
            ) -> GetResult<String, ()> {
                match self.players.iter().find(|(id, _)| *id == index_chain[0].0) {
                    Some((_, title)) => GetResult::Value(title.clone()),
                    None => GetResult::Unimplemented,
                }
            }

            async fn media_players_playback_play(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                _index_chain: [UNib32; 1],
            ) -> RpcResult<()> {
                RpcResult::Ready(())
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                observe_arrays_api :: Arrays for super::Server,
                server = true, no_alloc = false, use_async = true,
                method_model = "_=immediate",
                property_model = "zoom=value_on_changed, _=get_set",
                // two elements of a level observed at a time
                observe_elements = 2,
                // debug_to_file = "../../target/tests_observe_arrays_server.rs"
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
                self.now_us += 1_000;
                let mut buf = core::mem::take(&mut self.buf);
                _ = self.poll_properties(out, (), self.now_us, &mut buf).await;
                self.buf = buf;
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
                observe_arrays_api :: Arrays for super::Client,
                client = "std_client",
            );
        }
    }

    use client::Client;

    const MS: Duration = Duration::from_millis(1);
    /// A hang detector: everything here comes within a few polls
    const WAIT: Duration = Duration::from_secs(5);

    struct Bench {
        _device: TestDevice,
        client: Client,
    }

    async fn start(path: &str) -> Bench {
        let _ = tracing_subscriber::fmt::try_init();
        let server = server::Server {
            browser_state: [0; 3],
            browsers_zoom: [0; 3],
            players: vec![(5, "five".into()), (9, "nine".into())],
            player_ids: vec![UNib32(5), UNib32(9)],
            observe: server::api_impl::ObserveState::new(
                ww_stream::observe::ObserveLimits::DEFAULT,
            ),
            now_us: 0,
            buf: vec![0; 512],
        };
        let device = tests_common::start_device_async(
            path,
            server,
            observe_arrays_api::ARRAYS_FULL_GID,
            observe_arrays_api::ARRAYS_WIRE_VERSION,
            server::api_impl::api_hash(),
        );
        let client = Client::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");
        Bench {
            _device: device,
            client,
        }
    }

    impl Bench {
        /// Returns after everything the device sent before it answered.
        async fn barrier(&self) {
            self.client.barrier().call().await.unwrap();
        }
    }

    async fn recv<U, T>(o: &mut wire_weaver_client::observe::Observer<U, T>) -> ObserveEvent<T>
    where
        U: wire_weaver_client::observe::LevelUpdate,
    {
        tokio::time::timeout(WAIT, o.recv_event())
            .await
            .expect("an event")
            .expect("open")
    }

    async fn value<U, T: std::fmt::Debug>(o: &mut wire_weaver_client::observe::Observer<U, T>) -> T
    where
        U: wire_weaver_client::observe::LevelUpdate,
    {
        match recv(o).await {
            ObserveEvent::Update(u) => u.value,
            ev => panic!("an update, got {ev:?}"),
        }
    }

    /// Two elements of the same level, each observed by its own path: a change of one reaches only its observers.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn each_element_gets_its_own_updates() {
        let b = start("observe_arrays/own_updates").await;
        let mut one = b
            .client
            .browsers(1)
            .observe_state(on_change(MS))
            .subscribe()
            .await
            .unwrap();
        let mut two = b
            .client
            .browsers(2)
            .observe_state(on_change(MS))
            .subscribe()
            .await
            .unwrap();
        // the value to start from
        assert_eq!(value(&mut one).await, 0);
        assert_eq!(value(&mut two).await, 0);

        b.client.set_browser_state(1, 11).call().await.unwrap();
        assert_eq!(value(&mut one).await, 11);
        b.client.set_browser_state(2, 22).call().await.unwrap();
        assert_eq!(value(&mut two).await, 22);
        b.barrier().await;
        assert!(one.try_recv().unwrap().is_none(), "2's change went to 1");

        // a change of an element nobody observes sends nothing
        b.client.set_browser_state(0, 3).call().await.unwrap();
        b.barrier().await;
        assert!(one.try_recv().unwrap().is_none());
        assert!(two.try_recv().unwrap().is_none());
    }

    /// A level under an array inside another trait, and one trait (`Playback`) under two arrays: each path is its
    /// own level on the device and on the host.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn nested_levels_and_a_trait_under_two_arrays() {
        let b = start("observe_arrays/nested").await;
        let mut browser = b
            .client
            .browsers(0)
            .playback()
            .observe_title(on_change(MS))
            .subscribe()
            .await
            .unwrap();
        let mut five = b
            .client
            .media()
            .players(5)
            .playback()
            .observe_title(on_change(MS))
            .subscribe()
            .await
            .unwrap();
        let mut nine = b
            .client
            .media()
            .players(9)
            .playback()
            .observe_title(on_change(MS))
            .subscribe()
            .await
            .unwrap();
        assert_eq!(value(&mut browser).await, "browser 0");
        assert_eq!(value(&mut five).await, "five");
        assert_eq!(value(&mut nine).await, "nine");

        b.client.set_title(9, "nine again".into()).call().await.unwrap();
        assert_eq!(value(&mut nine).await, "nine again");
        b.barrier().await;
        assert!(five.try_recv().unwrap().is_none());
        assert!(browser.try_recv().unwrap().is_none());

        // a flat level next to them, as before
        let mut volume = b
            .client
            .observe_volume(on_change(MS))
            .subscribe()
            .await
            .unwrap();
        assert_eq!(value(&mut volume).await, 7);
    }

    /// A host's write to an element's `value_on_changed` property marks that element changed (SRV-57).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn host_write_marks_its_element() {
        let b = start("observe_arrays/host_write").await;
        let mut zoom1 = b
            .client
            .browsers(1)
            .observe_zoom(on_change(MS))
            .subscribe()
            .await
            .unwrap();
        let mut zoom0 = b
            .client
            .browsers(0)
            .observe_zoom(on_change(MS))
            .subscribe()
            .await
            .unwrap();
        assert_eq!(value(&mut zoom1).await, 0);
        assert_eq!(value(&mut zoom0).await, 0);
        b.client.browsers(1).write_zoom(4).write().await.unwrap();
        assert_eq!(value(&mut zoom1).await, 4);
        b.barrier().await;
        assert!(zoom0.try_recv().unwrap().is_none());
    }

    /// Elements come and go: a removed one ends its observers on the host (`Ended`, then closed), and observing
    /// it again is refused until it is back.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn removed_element_ends_its_observers() {
        let b = start("observe_arrays/removed").await;
        let mut five = b
            .client
            .media()
            .players(5)
            .playback()
            .observe_title(on_change(MS))
            .subscribe()
            .await
            .unwrap();
        let mut nine = b
            .client
            .media()
            .players(9)
            .playback()
            .observe_title(on_change(MS))
            .subscribe()
            .await
            .unwrap();
        assert_eq!(value(&mut five).await, "five");
        assert_eq!(value(&mut nine).await, "nine");

        b.client.remove_player(5).call().await.unwrap();
        assert_eq!(recv(&mut five).await, ObserveEvent::Ended);
        let closed = tokio::time::timeout(WAIT, five.recv_event()).await.unwrap();
        assert!(matches!(closed, Err(StreamError::Closed)), "{closed:?}");
        let e = five
            .configure(Default::default())
            .await
            .expect_err("ended");
        assert!(matches!(e, Error::User(_)), "{e:?}");
        // the other element goes on
        b.client.set_title(9, "still here".into()).call().await.unwrap();
        assert_eq!(value(&mut nine).await, "still here");

        // gone: its index isn't valid
        let e = b
            .client
            .media()
            .players(5)
            .playback()
            .observe_title(on_change(MS))
            .subscribe()
            .await
            .err()
            .expect("no player 5");
        assert!(format!("{e:?}").contains("BadIndex"), "{e:?}");

        // back: observed anew
        b.client.add_player(5).call().await.unwrap();
        let mut five = b
            .client
            .media()
            .players(5)
            .playback()
            .observe_title(on_change(MS))
            .subscribe()
            .await
            .unwrap();
        assert_eq!(value(&mut five).await, "new 5");
        b.client.set_title(5, "five again".into()).call().await.unwrap();
        assert_eq!(value(&mut five).await, "five again");
    }

    /// An index the device doesn't have is refused like any request to it; a level takes as many elements as it
    /// has tables (`observe_elements = 2`), a freed table takes the next one.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bad_index_and_full_tables() {
        let b = start("observe_arrays/full").await;
        let e = b
            .client
            .browsers(7)
            .observe_state(on_change(MS))
            .subscribe()
            .await
            .err()
            .expect("no browser 7");
        assert!(format!("{e:?}").contains("BadIndex"), "{e:?}");

        let zero = b
            .client
            .browsers(0)
            .observe_state(on_change(MS))
            .subscribe()
            .await
            .unwrap();
        let _one = b
            .client
            .browsers(1)
            .observe_state(on_change(MS))
            .subscribe()
            .await
            .unwrap();
        let e = b
            .client
            .browsers(2)
            .observe_state(on_change(MS))
            .subscribe()
            .await
            .err()
            .expect("both tables in use");
        assert!(format!("{e:?}").contains("OperationNotSupported"), "{e:?}");
        // the other levels have tables of their own
        b.client
            .browsers(2)
            .playback()
            .observe_title(on_change(MS))
            .subscribe()
            .await
            .unwrap();

        // the last observer of browser 0 gone: the device turns it off, its table is free
        drop(zero);
        b.barrier().await;
        let mut two = b
            .client
            .browsers(2)
            .observe_state(on_change(MS))
            .subscribe()
            .await
            .unwrap();
        assert_eq!(value(&mut two).await, 0);
    }

    /// Updates come on each element's own path.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn updates_carry_the_element_path() {
        use ww_client_server::{Event, EventKind};
        let b = start("observe_arrays/paths").await;
        let (trace_tx, mut trace_rx) = tokio::sync::mpsc::unbounded_channel();
        b.client
            .cmd
            .send(wire_weaver_client::internal::Command::RegisterTracer {
                trace_event_tx: trace_tx,
            })
            .await
            .unwrap();
        let mut a = b
            .client
            .browsers(1)
            .playback()
            .observe_title(on_change(MS))
            .subscribe()
            .await
            .unwrap();
        let mut c = b
            .client
            .media()
            .players(9)
            .playback()
            .observe_title(on_change(MS))
            .subscribe()
            .await
            .unwrap();
        value(&mut a).await;
        value(&mut c).await;
        // the tracer gets the events on its own way: wait for both
        let mut paths = BTreeSet::new();
        while paths.len() < 2 {
            let ev = tokio::time::timeout(WAIT, trace_rx.recv())
                .await
                .expect("traced updates")
                .unwrap();
            let wire_weaver_client::internal::TraceEvent::Event { bytes } = ev else {
                continue;
            };
            if let Ok(Event {
                result: Ok(EventKind::PropertyUpdate { path, .. }),
                ..
            }) = Event::from_ww_bytes(&bytes)
            {
                paths.insert(path.iter().map(|p| p.unwrap().0).collect::<Vec<_>>());
            }
        }
        // browsers is item 0 of the root, playback item 2 of Browser; media item 1, players item 0, playback item 1
        assert_eq!(
            paths,
            BTreeSet::from([vec![0, 1, 2], vec![1, 0, 9, 1]]),
            "{paths:?}"
        );
    }

    /// The dynamic client (TOOL-11) observes an element by its path from introspection alone, and ends with it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dynamic_client_observes_an_element() {
        use std::collections::BTreeMap;
        use wire_weaver_client::DynResource;
        use wire_weaver_client::ww_self::ValueOwned;
        let b = start("observe_arrays/dynamic").await;
        let bundle = b
            .client
            .cmd
            .device_introspect()
            .expect("introspection enabled")
            .api_bundle
            .clone();
        let playback = DynResource::root(bundle)
            .child("media")
            .unwrap()
            .child("players")
            .unwrap()
            .index(9)
            .unwrap()
            .child("playback")
            .unwrap();
        let mut observer = playback
            .observe(&b.client.cmd, BTreeMap::from([(0, on_change(MS))]))
            .await
            .unwrap();
        let first = tokio::time::timeout(WAIT, observer.recv()).await.unwrap().unwrap();
        assert_eq!(
            first.values,
            [("title".to_string(), ValueOwned::String("nine".into()))]
        );
        b.client.remove_player(9).call().await.unwrap();
        let closed = tokio::time::timeout(WAIT, observer.recv()).await.unwrap();
        assert!(matches!(closed, Err(StreamError::Closed)), "{closed:?}");
    }
}

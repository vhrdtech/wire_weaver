//! The server's call gate (SRV-50) end to end: a server generated with `call_gate = true` on `ww_device::Server`
//! through `tests_common::start_device`, the generated client through the real host event loop. While guarded the
//! gate admits `ww_user_info::UserInfo` and `motor.stop` and refuses the rest, streams and sinks at open; the guard
//! flips at run time. Observe and multi requests go to `process_request_bytes` as raw requests.

#[cfg(test)]
mod tests {
    use call_gate_api::MotorCommand;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use tests_common::TestDevice;
    use wire_weaver::gate::{Op, Target, TraitId};
    use wire_weaver::prelude::*;
    use wire_weaver_client::Error;

    const REASON: &str = "board guarded: unknown rework 9";
    const USER_INFO: TraitId = TraitId::new("ww_user_info", "UserInfo");
    const MOTOR: TraitId = TraitId::new("call_gate_api", "Motor");

    /// What the device saw: the gate's questions and the handlers that ran.
    #[derive(Default)]
    struct Seen {
        guarded: AtomicBool,
        /// `motor.start` handler runs
        starts: AtomicU32,
        /// `motor.stop` handler runs
        stops: AtomicU32,
        /// `motor.events` opens that reached the handler
        opens: AtomicU32,
        /// `motor.commands` writes that reached the handler
        commands: AtomicU32,
        /// `channel[].pulse` handler runs
        pulses: AtomicU32,
        /// Every target the gate was asked about, with the op
        asked: std::sync::Mutex<Vec<(Target, Op)>>,
    }

    mod server {
        use super::*;
        use tests_common::TestProcessEvents;
        use wire_weaver::BlockingEventOut;
        use wire_weaver::gate::CallGate;

        pub struct Server {
            pub seen: Arc<Seen>,
            pub name: String,
            pub observe: api_impl::ObserveState,
        }

        /// The firmware's side of the gate: while guarded, the info trait and `motor.stop` only.
        impl CallGate for Server {
            fn admit(&self, target: &Target, op: Op) -> Result<(), &str> {
                self.seen.asked.lock().unwrap().push((*target, op));
                if !self.seen.guarded.load(Ordering::SeqCst)
                    || target.is_in(&USER_INFO)
                    || target.is(&MOTOR, "stop")
                {
                    Ok(())
                } else {
                    Err(REASON)
                }
            }
        }

        impl Server {
            fn set_user_name(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: MaxLen<&'_ str, 32>,
            ) -> SetResult<ww_user_info::UserInfoError<'_>> {
                self.name = value.to_string();
                Set
            }

            fn get_user_name(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<MaxLen<&'_ str, 32>, ww_user_info::UserInfoError<'_>> {
                Value(MaxLen::new_unchecked(&self.name))
            }

            fn set_user_notes(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _value: MaxLen<&'_ str, 256>,
            ) -> SetResult<ww_user_info::UserInfoError<'_>> {
                Unimplemented.into()
            }

            fn get_user_notes(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<MaxLen<&'_ str, 256>, ww_user_info::UserInfoError<'_>> {
                Unimplemented.into()
            }

            fn motor_start(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<()> {
                self.seen.starts.fetch_add(1, Ordering::SeqCst);
                Ready(())
            }

            fn motor_stop(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<()> {
                self.seen.stops.fetch_add(1, Ordering::SeqCst);
                Ready(())
            }

            fn set_motor_enabled(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _value: bool,
            ) -> SetResult<()> {
                Set
            }

            fn get_motor_enabled(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<bool, ()> {
                Value(true)
            }

            fn get_motor_running(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<bool, ()> {
                Value(false)
            }

            fn motor_events(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<()> {
                self.seen.opens.fetch_add(1, Ordering::SeqCst);
                Ready(())
            }

            fn close_motor_events(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn sideband_motor_events(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }

            fn sideband_motor_commands(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }

            fn write_motor_commands(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _value: MotorCommand,
            ) {
                self.seen.commands.fetch_add(1, Ordering::SeqCst);
            }

            fn valid_indices_root_channel(&mut self) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..4)
            }

            fn channel_pulse(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _index_chain: [UNib32; 1],
            ) -> RpcResult<()> {
                self.seen.pulses.fetch_add(1, Ordering::SeqCst);
                Ready(())
            }

            fn observe_state(&mut self) -> &mut api_impl::ObserveState {
                &mut self.observe
            }
        }

        impl api_impl::CallGateTestConsts for Server {
            const USER_CAPABILITIES: ww_user_info::UserInfoCaps = ww_user_info::UserInfoCaps {
                name_max: 32,
                notes_max: 0,
                persistent: false,
            };
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                call_gate_api :: CallGateTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                multi_req = true,
                call_gate = true,
                // debug_to_file = "../../target/tests_call_gate_server.rs"
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

    mod std_client {
        use wire_weaver_client::{ClientConfig, Commander, WwClient};

        pub struct StdClient {
            pub cmd: Commander,
        }

        impl WwClient for StdClient {
            fn default_config() -> ClientConfig {
                ClientConfig::default()
            }

            fn from_cmd(cmd: Commander) -> Self {
                Self { cmd }
            }
        }

        mod api_client {
            wire_weaver::ww_codegen!(
                call_gate_api :: CallGateTest for super::StdClient,
                client = "std_client",
                // debug_to_file = "../../target/tests_call_gate_client.rs"
            );
        }
    }

    use std_client::StdClient;

    fn new_server(seen: &Arc<Seen>) -> server::Server {
        server::Server {
            seen: seen.clone(),
            name: "bench".into(),
            observe: server::api_impl::ObserveState::new(
                ww_stream::observe::ObserveLimits::DEFAULT,
            ),
        }
    }

    fn start(path: &str, guarded: bool) -> (TestDevice, Arc<Seen>) {
        let _ = tracing_subscriber::fmt::try_init();
        let seen = Arc::new(Seen::default());
        seen.guarded.store(guarded, Ordering::SeqCst);
        let device = tests_common::start_device(
            path,
            new_server(&seen),
            call_gate_api::CALL_GATE_TEST_FULL_GID,
            server::api_impl::api_hash(),
        );
        (device, seen)
    }

    async fn connect(device: &TestDevice) -> StdClient {
        StdClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    fn assert_refused<T: std::fmt::Debug>(r: Result<T, Error>) {
        match r {
            Err(Error::Refused { reason, err_seq }) => {
                assert_eq!(reason, REASON);
                assert_eq!(err_seq, ww_client_server::err_seq::GATE);
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    fn count(c: &AtomicU32) -> u32 {
        c.load(Ordering::SeqCst)
    }

    /// While guarded, the admitted trait and item are served, every other item is refused before its handler runs:
    /// calls, property reads and writes, the valid indices of an array, an array item, a stream open and sink writes.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn guarded_serves_only_what_it_admits() {
        let (device, seen) = start("call_gate/guarded", true);
        let client = connect(&device).await;

        // the admitted global trait, read and written
        assert_eq!(
            client.user().read_name().read().await.unwrap().as_str(),
            "bench"
        );
        client
            .user()
            .write_name(MaxLen::new("guarded".to_string()).unwrap())
            .write()
            .await
            .unwrap();
        assert_eq!(
            client.user().read_name().read().await.unwrap().as_str(),
            "guarded"
        );
        let caps = client.user().read_CAPABILITIES().read().await.unwrap();
        assert_eq!(caps.name_max, 32);
        // the admitted item of a refused trait
        client.motor().stop().call().await.unwrap();
        assert_eq!(count(&seen.stops), 1);

        assert_refused(client.motor().start().call().await);
        assert_refused(client.motor().read_enabled().read().await);
        assert_refused(client.motor().write_enabled(false).write().await);
        assert_refused(client.motor().read_running().read().await);
        assert_refused(client.channel(1).pulse().call().await);
        assert_refused(client.channel_valid_indices().read().await);
        assert_refused(client.motor().events().call().await.map(|_| ()));
        assert_eq!(count(&seen.starts), 0);
        assert_eq!(count(&seen.pulses), 0);
        assert_eq!(count(&seen.opens), 0, "a refused stream doesn't open");

        // a sink write has no answer: refused all the same, the handler never sees it
        let mut sink = client.motor().commands().await.unwrap();
        sink.send(MotorCommand::Start).await.unwrap();
        // a call answered after the write: the device took the write before it
        client.motor().stop().call().await.unwrap();
        assert_eq!(count(&seen.commands), 0);

        // what the gate was asked: the item, its trait, its path and what the request does
        let asked = seen.asked.lock().unwrap().clone();
        let has = |path: &str, op: Op| asked.iter().any(|(t, o)| t.path == path && *o == op);
        for (path, op) in [
            ("user.name", Op::Read),
            ("user.name", Op::Write),
            ("user.CAPABILITIES", Op::Read),
            ("motor.start", Op::Call),
            ("motor.enabled", Op::Write),
            ("channel.pulse", Op::Call),
            ("channel", Op::Read),
            ("motor.events", Op::StreamOpen),
        ] {
            assert!(has(path, op), "{path} {op:?} not asked: {asked:?}");
        }
        let start = asked
            .iter()
            .find(|(t, _)| t.path == "motor.start")
            .unwrap()
            .0;
        assert_eq!(*start.trait_id, MOTOR);
        assert_eq!(start.item(), Some("start"));
        let indices = asked.iter().find(|(t, _)| t.path == "channel").unwrap().0;
        assert_eq!(
            *indices.trait_id,
            TraitId::new("call_gate_api", "CallGateTest"),
            "an array is an item of the trait it is defined in"
        );
        let name = asked.iter().find(|(t, _)| t.path == "user.name").unwrap().0;
        assert_eq!(*name.trait_id, USER_INFO);
    }

    /// The gate is asked on every request, so the device's state decides at once: refused while guarded, served
    /// again after.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_gate_follows_the_state_at_run_time() {
        let (device, seen) = start("call_gate/flips", false);
        let client = connect(&device).await;
        client.motor().start().call().await.unwrap();
        let _stream = client.motor().events().call().await.unwrap();
        assert_eq!(count(&seen.opens), 1);

        seen.guarded.store(true, Ordering::SeqCst);
        assert_refused(client.motor().start().call().await);
        assert_refused(client.motor().events().call().await.map(|_| ()));
        assert_eq!((count(&seen.starts), count(&seen.opens)), (1, 1));

        seen.guarded.store(false, Ordering::SeqCst);
        client.motor().start().call().await.unwrap();
        client.channel(2).pulse().call().await.unwrap();
        assert_eq!((count(&seen.starts), count(&seen.pulses)), (2, 1));
        let mut sink = client.motor().commands().await.unwrap();
        sink.send(MotorCommand::Stop).await.unwrap();
        // answered after the write
        client.motor().stop().call().await.unwrap();
        assert_eq!(
            count(&seen.commands),
            1,
            "a sink write reaches its handler when admitted"
        );
    }

    mod raw {
        //! Observe and multi requests, straight into `process_request_bytes`.
        use super::*;
        use wire_weaver::shrink_wrap::either_any_vec::EitherAnyVec;
        use wire_weaver::shrink_wrap::{BufReader, RefVec, UVlq32Backfill};
        use ww_client_server::observe::{ObserveEntry, ObserveMode, ObserveRequest};
        use ww_client_server::{
            ErrorKind, ErrorKindOwned, ErrorOwned, Event, EventKind, MultiArgs, MultiIndex,
            PathKind, Request, RequestKind,
        };

        /// `motor` in the root trait, `running` in `Motor`, `channel` in the root trait, `pulse` in `Channel`
        const MOTOR_ID: u32 = 1;
        const RUNNING: u32 = 3;
        const CHANNEL_ID: u32 = 2;
        const PULSE: u32 = 0;

        fn process(server: &mut server::Server, path: &[UNib32], kind: RequestKind<'_>) -> Vec<u8> {
            let req = Request {
                seq: UVlq32Backfill(1),
                priority: ww_client_server::Priority::NORMAL.nibble(),
                path_kind: PathKind::Absolute {
                    path: RefVec::Slice { slice: path },
                },
                kind,
            };
            let mut buf = [0u8; 128];
            let req = req.to_ww_bytes(&mut buf).unwrap().to_vec();
            let mut scratch = [0u8; 256];
            let mut queue_buf = [0u8; 256];
            let mut queue = wire_weaver::EventQueue::new(&mut queue_buf);
            server
                .process_request_bytes(&req, &mut scratch, &mut queue, ())
                .unwrap()
                .to_vec()
        }

        fn observe_running(server: &mut server::Server) -> Vec<u8> {
            let entries = [ObserveEntry {
                index: UNib32(RUNNING),
                mode: ObserveMode::OnChange {
                    min_us: UNib32(1_000),
                },
            }];
            let cfg = ObserveRequest {
                entries: RefVec::Slice { slice: &entries },
                patch: false,
                budget: None,
            };
            process(server, &[UNib32(MOTOR_ID)], RequestKind::Observe { cfg })
        }

        /// An observe request asks the gate for the observed trait level, refused with its own site.
        #[test]
        fn observe_is_asked_for_the_level() {
            let seen = Arc::new(Seen::default());
            let mut server = new_server(&seen);
            seen.guarded.store(true, Ordering::SeqCst);
            let reply = observe_running(&mut server);
            let reply = Event::from_ww_bytes(&reply).unwrap();
            let Err(e) = reply.result else {
                panic!("{reply:?}")
            };
            assert!(
                matches!(e.kind(), ErrorKind::Refused(r) if *r == REASON),
                "{e:?}"
            );
            assert_eq!(e.err_seq(), ww_client_server::err_seq::OBSERVE_GATE);
            let (target, op) = seen.asked.lock().unwrap()[0];
            assert_eq!(
                target,
                Target {
                    trait_id: &MOTOR,
                    path: "motor",
                    level: true,
                }
            );
            assert_eq!(op, Op::Observe);

            seen.guarded.store(false, Ordering::SeqCst);
            let reply = observe_running(&mut server);
            let reply = Event::from_ww_bytes(&reply).unwrap();
            assert!(
                matches!(reply.result, Ok(EventKind::Value { .. })),
                "{reply:?}"
            );
        }

        /// Each index of a multi call asks the gate on its own: refused in its slot, the handler doesn't run.
        #[test]
        fn multi_call_is_asked_per_index() {
            let seen = Arc::new(Seen::default());
            let mut server = new_server(&seen);
            seen.guarded.store(true, Ordering::SeqCst);
            let indices = [UNib32(0), UNib32(3)];
            let kind = RequestKind::MultiCall {
                multi_idx: MultiIndex::List(RefVec::Slice { slice: &indices }),
                in_each_array_id: Some(UNib32(PULSE)),
                multi_args: MultiArgs::Same(RefVec::Slice { slice: &[] }),
            };
            let reply = process(&mut server, &[UNib32(CHANNEL_ID)], kind);
            let reply = Event::from_ww_bytes(&reply).unwrap();
            let Ok(EventKind::Value { data }) = reply.result else {
                panic!("{reply:?}")
            };
            let mut rd = BufReader::new(data.0);
            let vec = EitherAnyVec::des_shrink_wrap(&mut rd).unwrap();
            let mut iter = vec.iter();
            for _ in 0..2 {
                let item: Result<(), ErrorOwned> =
                    iter.next_owned::<ErrorOwned, ()>().unwrap().into();
                let Err(e) = item else { panic!("{item:?}") };
                assert!(
                    matches!(&e.kind, ErrorKindOwned::Refused(r) if r == REASON),
                    "{e:?}"
                );
            }
            assert_eq!(count(&seen.pulses), 0);
            let asked = seen.asked.lock().unwrap();
            assert_eq!(asked.len(), 2);
            assert!(
                asked
                    .iter()
                    .all(|(t, op)| t.path == "channel.pulse" && *op == Op::Call)
            );
        }
    }
}

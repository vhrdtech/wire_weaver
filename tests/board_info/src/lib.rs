//! `ww_board_info` end to end: a mock server on `ww_device::Server` through `tests_common::start_device`, the generated
//! client through the real host event loop.

#[cfg(test)]
mod tests {
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use ww_board_info::{
        BoardSummary, GuardState, ImageId, RevisionSource, ReworkChange, ReworkError,
        ReworkErrorOwned, ReworkId, ReworkState,
        guard::{self, GuardReport, StoredAllowance},
        table::{ReworkDef, ReworkSet, ReworkTable},
    };

    /// The running image's id: the first 8 bytes of its build id.
    const OWN: ImageId = ImageId([0x3f, 0xa2, 1, 2, 3, 4, 5, 6]);
    /// Another image's.
    const OTHER: ImageId = ImageId([0x91, 0xc0, 1, 2, 3, 4, 5, 6]);

    /// The firmware's rework table: rework 6 of the board's list is no rework, 2 and 4 rewire the same lines.
    const TABLE: ReworkTable = ReworkTable::new(
        &[
            ReworkDef::new(1, "0R to 10k on R12"),
            ReworkDef::new(2, "swap SDA and SCL"),
            ReworkDef::new(3, "cut trace to U3 pin 4"),
            ReworkDef::new(4, "SDA and SCL to the other header"),
        ],
        &[(ReworkId(2), ReworkId(4))],
    );

    mod server {
        use super::*;
        use std::sync::{Arc, Mutex};
        use tests_common::TestProcessEvents;
        use wire_weaver::BlockingEventOut;
        use ww_board_info::Rework;

        /// A board with the given revision and where it came from.
        pub struct Server {
            pub revision_source: RevisionSource,
            pub known: bool,
            /// The unit's record: a bit per rework, shared with the test
            pub record: Arc<Mutex<ReworkSet>>,
            /// A production record fixes the reworks
            pub production: bool,
            /// `REWORKS` as served: the table and the record at boot
            pub list: Vec<Rework<'static>>,
            /// The stored allowance, shared with the test
            pub allowance: Arc<Mutex<StoredAllowance>>,
            /// `GUARD` as served: decided at boot
            pub guard: GuardReport,
        }

        impl Server {
            pub fn new(revision_source: RevisionSource, known: bool, applied: ReworkSet) -> Self {
                Self::boot(revision_source, known, applied, StoredAllowance::NONE)
            }

            /// Boot as a firmware does: decide, erase an allowance for another image.
            pub fn boot(
                revision_source: RevisionSource,
                known: bool,
                applied: ReworkSet,
                mut allowance: StoredAllowance,
            ) -> Self {
                let decision = guard::decide(TABLE.known(), applied, allowance, Some(OWN));
                if decision.erase_allowance {
                    allowance = StoredAllowance::NONE;
                }
                Server {
                    revision_source,
                    known,
                    record: Arc::new(Mutex::new(applied)),
                    production: false,
                    list: TABLE.list(applied).collect(),
                    allowance: Arc::new(Mutex::new(allowance)),
                    guard: GuardReport::new(decision.guard, allowance, Some(OWN)),
                }
            }

            fn board_allow_unknown_reworks(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                image: ImageId,
                reworks: MaxLen<RefVec<'_, ReworkId>, 16>,
                program: bool,
            ) -> RpcResult<Result<ReworkChange, ReworkError<'_>>> {
                let new = match guard::allowance_from_request(
                    image,
                    reworks.iter().map_while(Result::ok),
                ) {
                    Ok(new) => new,
                    Err(id) => return Ready(Err(ReworkError::UnknownRework { id })),
                };
                Ready(Ok(store(&self.allowance, new, program)))
            }

            fn board_clear_unknown_reworks_allowance(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                program: bool,
            ) -> RpcResult<Result<ReworkChange, ReworkError<'_>>> {
                Ready(Ok(store(&self.allowance, StoredAllowance::NONE, program)))
            }

            fn board_set_reworks(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                changes: MaxLen<RefVec<'_, ReworkState>, 16>,
                program: bool,
            ) -> RpcResult<Result<ReworkChange, ReworkError<'_>>> {
                // guarded: reading the record only
                if let Some(id) = self.guard.guard.unknown().iter().next()
                    && !self.guard.guard.runs_board()
                {
                    return Ready(Err(ReworkError::Guarded { id }));
                }
                if self.production {
                    return Ready(Err(ReworkError::ProductionRecord));
                }
                let changes = changes.iter().map_while(Result::ok);
                let mut record = self.record.lock().unwrap();
                let applied = match TABLE.apply(*record, changes) {
                    Ok(applied) => applied,
                    Err(e) => return Ready(Err(e)),
                };
                Ready(Ok(if applied == *record {
                    ReworkChange::Unchanged
                } else if !program {
                    ReworkChange::WouldWrite
                } else {
                    *record = applied;
                    ReworkChange::Written
                }))
            }
        }

        fn store(
            stored: &Mutex<StoredAllowance>,
            new: StoredAllowance,
            program: bool,
        ) -> ReworkChange {
            let mut stored = stored.lock().unwrap();
            if *stored == new {
                ReworkChange::Unchanged
            } else if !program {
                ReworkChange::WouldWrite
            } else {
                *stored = new;
                ReworkChange::Written
            }
        }

        /// The board record is read from OTP at boot: a `fn` (`const_model`), served without a handler.
        impl api_impl::BoardInfoTestConsts for Server {
            fn board_board(&self) -> BoardSummary<'_> {
                // what the device doesn't know is empty or None
                BoardSummary {
                    name: MaxLen::new_unchecked("B200"),
                    revision: MaxLen::new_unchecked("B"),
                    revision_source: self.revision_source,
                    variant: MaxLen::new_unchecked(if self.known { "usb-io" } else { "" }),
                    bom_variant: MaxLen::new_unchecked(""),
                    serial: MaxLen::new_unchecked(if self.known { "B200-0042" } else { "" }),
                    build_date: self.known.then(|| {
                        ww_board_info::DateTime::from_ymd_hms_utc_opt(2026, 9, 1, 0, 0, 0, 0)
                            .unwrap()
                    }),
                }
            }

            /// Bring-up board: the table, with what the unit's record says applied.
            fn board_reworks(&self) -> MaxLen<RefVec<'_, Rework<'_>>, 16> {
                MaxLen::new_unchecked(RefVec::Slice { slice: &self.list })
            }

            fn board_guard(&self) -> ww_board_info::GuardSummary<'_> {
                self.guard.summary()
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                board_info_api :: BoardInfoTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                const_model = "BOARD|REWORKS|GUARD=fn",
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

    /// A device that doesn't keep a rework list: `REWORKS` is left out with `omit`.
    mod plain_server {
        use super::*;
        use tests_common::TestProcessEvents;
        use wire_weaver::BlockingEventOut;

        pub struct Server;

        impl api_impl::BoardInfoTestConsts for Server {
            fn board_board(&self) -> BoardSummary<'_> {
                BoardSummary {
                    name: MaxLen::new_unchecked("B200"),
                    revision: MaxLen::new_unchecked("A"),
                    revision_source: RevisionSource::Assumed,
                    variant: MaxLen::new_unchecked(""),
                    bom_variant: MaxLen::new_unchecked(""),
                    serial: MaxLen::new_unchecked(""),
                    build_date: None,
                }
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                board_info_api :: BoardInfoTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                const_model = "BOARD=fn",
                omit = [
                    "board.REWORKS",
                    "board.set_reworks",
                    "board.GUARD",
                    "board.allow_unknown_reworks",
                    "board.clear_unknown_reworks_allowance",
                ],
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
                board_info_api :: BoardInfoTest for super::StdClient,
                client = "std_client",
            );
        }
    }

    use std_client::StdClient;

    async fn connect(device: &TestDevice) -> StdClient {
        StdClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    fn start(path: &str, revision_source: RevisionSource, known: bool) -> TestDevice {
        start_server(path, server::Server::new(revision_source, known, set(&[1])))
    }

    fn start_server(path: &str, server: server::Server) -> TestDevice {
        let _ = tracing_subscriber::fmt::try_init();
        tests_common::start_device(
            path,
            server,
            board_info_api::BOARD_INFO_TEST_FULL_GID,
            server::api_impl::api_hash(),
        )
    }

    fn set(ids: &[u16]) -> ReworkSet {
        ids.iter()
            .fold(ReworkSet::EMPTY, |s, &id| s.with(ReworkId(id), true))
    }

    fn change(id: u16, applied: bool) -> ReworkState {
        ReworkState {
            id: ReworkId(id),
            applied,
        }
    }

    /// `REWORKS` as (number, title, applied).
    async fn reworks(client: &StdClient) -> Vec<(u16, Option<String>, bool)> {
        let list = client.board().read_REWORKS().read().await.unwrap();
        list.iter()
            .map(|r| (r.id.0, r.title.as_ref().map(|t| t.to_string()), r.applied))
            .collect()
    }

    async fn set_reworks(
        client: &StdClient,
        changes: &[ReworkState],
        program: bool,
    ) -> Result<ReworkChange, ReworkErrorOwned> {
        match client
            .board()
            .set_reworks(MaxLen::new_unchecked(changes.to_vec()), program)
            .call()
            .await
        {
            Ok(change) => Ok(change),
            Err(wire_weaver_client::CallError::Device(e)) => Err(e),
            Err(e) => panic!("set_reworks: {e:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn revision_from_record() {
        let device = start("board_info/record", RevisionSource::Record, true);
        let client = connect(&device).await;
        let b = client.board().read_BOARD().read().await.unwrap();
        assert_eq!(*b.name, "B200");
        assert_eq!(*b.revision, "B");
        assert_eq!(b.revision_source, RevisionSource::Record);
        assert_eq!(*b.variant, "usb-io");
        assert_eq!(*b.serial, "B200-0042");
        assert!(b.build_date.is_some());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn revision_from_strap_keeps_raw_value() {
        let device = start(
            "board_info/strap",
            RevisionSource::Strap { value: 0x2a5 },
            true,
        );
        let client = connect(&device).await;
        let b = client.board().read_BOARD().read().await.unwrap();
        assert_eq!(b.revision_source, RevisionSource::Strap { value: 0x2a5 });
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn assumed_revision_and_unknown_fields() {
        let device = start("board_info/assumed", RevisionSource::Assumed, false);
        let client = connect(&device).await;
        let b = client.board().read_BOARD().read().await.unwrap();
        assert_eq!(b.revision_source, RevisionSource::Assumed);
        assert!(b.variant.is_empty() && b.bom_variant.is_empty() && b.serial.is_empty());
        assert!(b.build_date.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reworks_round_trip() {
        let device = start("board_info/reworks", RevisionSource::Record, true);
        let client = connect(&device).await;
        let list = reworks(&client).await;
        assert_eq!(list.len(), 4);
        assert_eq!(list[0], (1, Some("0R to 10k on R12".into()), true));
        assert_eq!(list[2], (3, Some("cut trace to U3 pin 4".into()), false));
    }

    /// A record written by a newer firmware has reworks this one doesn't know: listed after the table, no title.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn applied_reworks_unknown_to_the_firmware() {
        let device = start_server(
            "board_info/unknown_applied",
            server::Server::new(RevisionSource::Provisional, true, set(&[3, 7])),
        );
        let client = connect(&device).await;
        let list = reworks(&client).await;
        assert_eq!(list.len(), 5);
        assert!(list[2].2);
        assert_eq!(list[4], (7, None, true));
        // the board is guarded: reading the record only
        assert_eq!(
            set_reworks(&client, &[change(7, false)], true).await,
            Err(ReworkErrorOwned::Guarded { id: ReworkId(7) })
        );
        drop(client);
        drop(device);
        // allowed for this image: undoing it is still refused, this firmware can't judge it
        let device = start_server(
            "board_info/unknown_applied_allowed",
            server::Server::boot(
                RevisionSource::Provisional,
                true,
                set(&[3, 7]),
                StoredAllowance::new(OWN, set(&[7])),
            ),
        );
        let client = connect(&device).await;
        assert_eq!(
            set_reworks(&client, &[change(7, false)], true).await,
            Err(ReworkErrorOwned::UnknownRework { id: ReworkId(7) })
        );
    }

    /// The record changes, `REWORKS` stays what the board booted with.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn set_dry_run_then_write() {
        let server = server::Server::new(RevisionSource::Provisional, true, set(&[1]));
        let record = server.record.clone();
        let device = start_server("board_info/set", server);
        let client = connect(&device).await;
        let three = [change(3, true)];
        assert_eq!(
            set_reworks(&client, &three, false).await,
            Ok(ReworkChange::WouldWrite)
        );
        assert_eq!(
            *record.lock().unwrap(),
            set(&[1]),
            "a dry run writes nothing"
        );
        assert_eq!(
            set_reworks(&client, &three, true).await,
            Ok(ReworkChange::Written)
        );
        assert_eq!(*record.lock().unwrap(), set(&[1, 3]));
        assert_eq!(
            set_reworks(&client, &three, true).await,
            Ok(ReworkChange::Unchanged)
        );
        assert_eq!(
            set_reworks(&client, &[change(1, false)], true).await,
            Ok(ReworkChange::Written)
        );
        assert_eq!(*record.lock().unwrap(), set(&[3]));
        assert!(reworks(&client).await[0].2, "REWORKS: the board as booted");
    }

    /// A number the firmware doesn't know is refused and nothing of the call is stored.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unknown_rework_is_refused() {
        let server = server::Server::new(RevisionSource::Provisional, true, set(&[1]));
        let record = server.record.clone();
        let device = start_server("board_info/unknown", server);
        let client = connect(&device).await;
        let err = set_reworks(&client, &[change(3, true), change(7, true)], true)
            .await
            .unwrap_err();
        assert_eq!(err, ReworkErrorOwned::UnknownRework { id: ReworkId(7) });
        assert_eq!(
            err.to_string(),
            "Unknown rework 7: this firmware doesn't know it, update the firmware first"
        );
        assert_eq!(
            *record.lock().unwrap(),
            set(&[1]),
            "nothing of the call stored"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn conflicting_reworks() {
        let server = server::Server::new(RevisionSource::Provisional, true, set(&[2]));
        let record = server.record.clone();
        let device = start_server("board_info/conflict", server);
        let client = connect(&device).await;
        assert_eq!(
            set_reworks(&client, &[change(4, true)], true).await,
            Err(ReworkErrorOwned::Conflict {
                id: ReworkId(4),
                with: ReworkId(2)
            })
        );
        // a swap in one call
        assert_eq!(
            set_reworks(&client, &[change(2, false), change(4, true)], true).await,
            Ok(ReworkChange::Written)
        );
        assert_eq!(*record.lock().unwrap(), set(&[4]));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn production_record_fixes_the_reworks() {
        let mut server = server::Server::new(RevisionSource::Record, true, set(&[1]));
        server.production = true;
        let device = start_server("board_info/production", server);
        let client = connect(&device).await;
        assert_eq!(
            set_reworks(&client, &[change(3, true)], false).await,
            Err(ReworkErrorOwned::ProductionRecord)
        );
    }

    /// A device that leaves `REWORKS` out: the client reports it without asking, BOARD still works.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn device_without_reworks() {
        let _ = tracing_subscriber::fmt::try_init();
        let device = tests_common::start_device(
            "board_info/plain",
            plain_server::Server,
            board_info_api::BOARD_INFO_TEST_FULL_GID,
            plain_server::api_impl::api_hash(),
        );
        let client = connect(&device).await;
        assert_eq!(
            *client.board().read_BOARD().read().await.unwrap().revision,
            "A"
        );
        let err = client.board().read_REWORKS().read().await.unwrap_err();
        assert!(
            matches!(
                err,
                wire_weaver_client::Error::NotImplementedByDevice { ref resource, .. }
                    if resource == "board.REWORKS"
            ),
            "{err:?}"
        );
        let err = client
            .board()
            .set_reworks(MaxLen::new_unchecked(vec![]), false)
            .call()
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                wire_weaver_client::CallError::Unimplemented { ref resource }
                    if resource == "board.set_reworks"
            ),
            "{err:?}"
        );
    }

    async fn allow(
        client: &StdClient,
        image: ImageId,
        ids: &[u16],
        program: bool,
    ) -> Result<ReworkChange, ReworkErrorOwned> {
        let ids = ids.iter().map(|&id| ReworkId(id)).collect();
        match client
            .board()
            .allow_unknown_reworks(image, MaxLen::new_unchecked(ids), program)
            .call()
            .await
        {
            Ok(change) => Ok(change),
            Err(wire_weaver_client::CallError::Device(e)) => Err(e),
            Err(e) => panic!("allow_unknown_reworks: {e:?}"),
        }
    }

    /// A record with a rework this firmware doesn't know: guarded, `set_reworks` refused, an allowance for the
    /// running image stored for the next boot, which then runs normally.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn guarded_then_allowed() {
        let server = server::Server::new(RevisionSource::Provisional, true, set(&[1, 7]));
        let allowance = server.allowance.clone();
        let device = start_server("board_info/guarded", server);
        let client = connect(&device).await;
        let g = client.board().read_GUARD().read().await.unwrap();
        assert_eq!(g.state, GuardState::Guarded);
        assert_eq!(g.unknown.iter().map(|id| id.0).collect::<Vec<_>>(), [7]);
        assert!(g.allowance.is_none());
        assert_eq!(
            set_reworks(&client, &[change(3, true)], true).await,
            Err(ReworkErrorOwned::Guarded { id: ReworkId(7) })
        );
        assert_eq!(
            allow(&client, OWN, &[7], false).await,
            Ok(ReworkChange::WouldWrite)
        );
        assert!(
            allowance.lock().unwrap().is_none(),
            "a dry run writes nothing"
        );
        assert_eq!(
            allow(&client, OWN, &[7], true).await,
            Ok(ReworkChange::Written)
        );
        assert_eq!(
            allow(&client, OWN, &[64], true).await,
            Err(ReworkErrorOwned::UnknownRework { id: ReworkId(64) })
        );
        let stored = *allowance.lock().unwrap();
        drop(client);
        drop(device);

        // next boot
        let server = server::Server::boot(RevisionSource::Provisional, true, set(&[1, 7]), stored);
        let device = start_server("board_info/allowed", server);
        let client = connect(&device).await;
        let g = client.board().read_GUARD().read().await.unwrap();
        assert_eq!(g.state, GuardState::Allowed);
        let a = g.allowance.unwrap();
        assert_eq!(a.image, OWN);
        assert!(a.for_running_image);
        assert_eq!(a.reworks.iter().map(|id| id.0).collect::<Vec<_>>(), [7]);
        assert_eq!(
            set_reworks(&client, &[change(3, true)], false).await,
            Ok(ReworkChange::WouldWrite)
        );
    }

    /// A flasher stores the allowance for the image it is about to write: the running image shows it as not its own;
    /// an image that finds another image's allowance at boot erases it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn allowance_for_another_image() {
        let server = server::Server::new(RevisionSource::Provisional, true, set(&[1]));
        let allowance = server.allowance.clone();
        let device = start_server("board_info/allow_other", server);
        let client = connect(&device).await;
        assert_eq!(
            client.board().read_GUARD().read().await.unwrap().state,
            GuardState::Normal
        );
        assert_eq!(
            allow(&client, OTHER, &[9], true).await,
            Ok(ReworkChange::Written)
        );
        let stored = *allowance.lock().unwrap();
        assert_eq!(stored.image_id(), Some(OTHER));
        // flashing failed, the same image boots again: the allowance isn't its own, it goes
        let server = server::Server::boot(RevisionSource::Provisional, true, set(&[1]), stored);
        assert!(server.allowance.lock().unwrap().is_none());
        // cleared by a flasher
        let clear = client
            .board()
            .clear_unknown_reworks_allowance(true)
            .call()
            .await
            .unwrap();
        assert_eq!(clear, ReworkChange::Written);
        assert!(allowance.lock().unwrap().is_none());
    }

    #[test]
    fn revision_source_wire() {
        // Strap carries a u16 after a one nibble discriminant
        let mut buf = [0u8; 8];
        let mut wr = BufWriter::new(&mut buf);
        wr.write(&RevisionSource::Strap { value: 0x2a5 }).unwrap();
        let bytes = wr.finish_and_take().unwrap();
        let mut rd = BufReader::new(bytes);
        assert_eq!(
            rd.read::<RevisionSource>().unwrap(),
            RevisionSource::Strap { value: 0x2a5 }
        );
    }
}

//! `ww info` / `ww report` end to end: a mock device on `ww_device::Server` through
//! `tests_common::start_device_async` implementing every device contract trait, queried through a traits-only
//! `DynClient` (any API, any version) by `ww_report`, counters and crash decoded with the cnt test firmware's ELF.
//!
//! The defmt table is built here (a host ELF can't carry a real `.defmt` section without the firmware's linker
//! script), as `tests/log_defmt` does.

#[cfg(test)]
mod tests {
    use object::{Object, ObjectSymbol, SymbolKind};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver_client::{ClientConfig, DynClient};
    use ww_board_info::{BoardSummary, RevisionSource};
    use ww_counters::{
        BufferInfo, CounterBuffer, CountersInfo, MAX_WORDS, read_words, reset_words,
    };
    use ww_defmt_logger::DefmtLogger;
    use ww_firmware_info::{
        BuildProfile, CortexMFault, Fault, FirmwareSummary, GitInfo, ResetCause, ResetRecord,
        Version,
    };
    use ww_report::clients::Contract;
    use ww_report::info::{Part, collect_info};
    use ww_report::report::{ReportOptions, collect_report, render_report};
    use ww_stream::store::{FullPolicy, RamRing};
    use ww_stream::tx::{Counted, Reliable};
    use ww_uid::{UidKind, UidSource, UniqueId};
    use ww_user_info::{UserInfoCaps, UserInfoError};

    const SAMPLE: u16 = 1;
    const COMMIT: [u8; 20] = [0xa1; 20];
    const RAM_WORDS: usize = 16;
    const RAM_USED: usize = 7;
    const BKP_USED: usize = 4;

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../counters/fixtures/counters_fw.elf")
    }

    fn fixture_build_id() -> Vec<u8> {
        cnt_core::elf::build_id(&std::fs::read(fixture()).unwrap())
            .unwrap()
            .unwrap()
    }

    /// An address inside a function of the fixture ELF, and its name.
    fn some_function() -> (u32, String) {
        let bytes = std::fs::read(fixture()).unwrap();
        let file = object::File::parse(&*bytes).unwrap();
        let s = file
            .symbols()
            .find(|s| s.kind() == SymbolKind::Text && s.size() > 8)
            .expect("a function in the fixture");
        (
            s.address() as u32 + 4,
            rustc_demangle::demangle(s.name().unwrap()).to_string(),
        )
    }

    /// The table the host would read from the firmware's ELF.
    fn table(encoding: &str) -> defmt_decoder::Table {
        let entry = |tag: &str, string: &str| serde_json::json!({ "string": { "tag": tag, "string": string }, "raw_symbol": "" });
        serde_json::from_value(serde_json::json!({
            "timestamp": entry("Timestamp", "{=u32}"),
            "entries": { SAMPLE.to_string(): entry("Info", "sample {=u32}") },
            "bitflags": {},
            "encoding": encoding,
        }))
        .expect("defmt_decoder::Table from JSON")
    }

    mod server {
        use super::*;
        use tests_common::TestProcessEventsAsync;
        use wire_weaver::EventOut;

        pub struct Server {
            pub logger: Box<DefmtLogger<512>>,
            pub defmt_frames_stream: Reliable<RamRing<4096>>,
            pub counters_watch_stream: Counted,
            pub stream_sessions: u32,
            pub build_id: Vec<u8>,
            pub crash_stack: Vec<u32>,
            pub crash_pc: u32,
            pub ram: Vec<AtomicU32>,
            pub bkp: Vec<AtomicU32>,
            pub scratch: [UVlq32; MAX_WORDS],
        }

        impl Server {
            pub async fn boot(build_id: Vec<u8>, crash_pc: u32) -> Self {
                let mut server = Server {
                    logger: Box::new(DefmtLogger::new()),
                    defmt_frames_stream: Reliable::new(
                        RamRing::new().with_policy(FullPolicy::DropOldest),
                    ),
                    counters_watch_stream: Counted::new(),
                    stream_sessions: 0,
                    build_id,
                    crash_stack: vec![crash_pc | 1, 0x2000_0100, crash_pc + 2],
                    crash_pc,
                    ram: (0..RAM_WORDS).map(|_| AtomicU32::new(0)).collect(),
                    bkp: (0..8).map(|_| AtomicU32::new(0)).collect(),
                    scratch: [UVlq32(0); MAX_WORDS],
                };
                // boots = 3
                server.ram[0].store(3, Ordering::Relaxed);
                let session = server.new_stream_session();
                server
                    .defmt_frames_stream
                    .start_session(session)
                    .await
                    .unwrap();
                server
            }

            pub fn log_frame(&self, value: u32) {
                self.logger.acquire();
                // safety: between acquire and release
                unsafe {
                    self.logger.write(&SAMPLE.to_le_bytes());
                    self.logger.write(&(value * 10).to_le_bytes());
                    self.logger.write(&value.to_le_bytes());
                    self.logger.release();
                }
            }

            pub async fn pump(&mut self) {
                self.logger
                    .pump(&mut self.defmt_frames_stream)
                    .await
                    .unwrap();
            }

            async fn log_samples(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                first: u32,
                count: u32,
            ) -> RpcResult<()> {
                for value in first..first + count {
                    self.log_frame(value);
                    self.pump().await;
                }
                RpcResult::Ready(())
            }

            async fn reset_last_reset(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
            ) -> RpcResult<ResetRecord<'_>> {
                RpcResult::Ready(ResetRecord {
                    boots_since_crash: 2,
                    cause: ResetCause::Panic,
                    build_id: MaxLen::new_unchecked(RefVec::new_bytes(&self.build_id)),
                    uptime: Some(ww_si::Second::milli(2500u32)),
                    fault: Some(Fault::CortexM(CortexMFault {
                        pc: self.crash_pc,
                        lr: self.crash_pc + 6,
                        xpsr: 0x6100_0000,
                        sp: 0x2000_0f00,
                        cfsr: 0,
                        hfsr: 0,
                        mmfar: 0,
                        bfar: 0,
                        exc_return: 0xffff_fff9,
                    })),
                    stack: MaxLen::new_unchecked(RefVec::Slice {
                        slice: &self.crash_stack,
                    }),
                    message: MaxLen::new_unchecked("index out of bounds"),
                })
            }

            async fn reset_boot_count(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
            ) -> RpcResult<u32> {
                RpcResult::Ready(3)
            }

            async fn reset_clear(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
                RpcResult::Ready(())
            }

            async fn set_user_name(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                _value: MaxLen<&'_ str, 32>,
            ) -> SetResult<UserInfoError<'_>> {
                Unimplemented.into()
            }

            async fn get_user_name(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
            ) -> GetResult<MaxLen<&'_ str, 32>, UserInfoError<'_>> {
                GetResult::Value(MaxLen::new_unchecked("bench 2"))
            }

            async fn set_user_notes(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                _value: MaxLen<&'_ str, 256>,
            ) -> SetResult<UserInfoError<'_>> {
                Unimplemented.into()
            }

            async fn get_user_notes(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
            ) -> GetResult<MaxLen<&'_ str, 256>, UserInfoError<'_>> {
                GetResult::Value(MaxLen::new_unchecked("DUT of the CAN test"))
            }

            async fn defmt_frames(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
            ) -> RpcResult<Result<(), ww_log::LogError<'_>>> {
                RpcResult::Ready(Ok(()))
            }

            async fn close_defmt_frames(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

            async fn stream_defmt_frames(&mut self) -> &mut Reliable<RamRing<4096>> {
                &mut self.defmt_frames_stream
            }

            async fn counters_read(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                buffer: CounterBuffer,
                offset: u16,
            ) -> RpcResult<
                Result<MaxLen<RefVec<'_, UVlq32>, MAX_WORDS>, ww_counters::CountersError<'_>>,
            > {
                let (words, used) = match buffer {
                    CounterBuffer::Ram => (&self.ram[..], RAM_USED),
                    CounterBuffer::Backup => (&self.bkp[..], BKP_USED),
                };
                RpcResult::Ready(read_words(words, used, offset, &mut self.scratch))
            }

            async fn counters_reset(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                buffer: CounterBuffer,
            ) -> RpcResult<Result<(), ww_counters::CountersError<'_>>> {
                match buffer {
                    CounterBuffer::Ram => reset_words(&self.ram),
                    CounterBuffer::Backup => reset_words(&self.bkp),
                }
                RpcResult::Ready(Ok(()))
            }

            async fn counters_watch(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                _period: ww_si::Second,
            ) -> RpcResult<Result<ww_si::Second, ww_counters::CountersError<'_>>> {
                RpcResult::Ready(Err(ww_counters::CountersError::unsupported()))
            }

            async fn close_counters_watch(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

            async fn stream_counters_watch(&mut self) -> &mut Counted {
                &mut self.counters_watch_stream
            }

            fn new_stream_session(&mut self) -> u32 {
                self.stream_sessions = self.stream_sessions.wrapping_add(1);
                self.stream_sessions
            }
        }

        impl api_impl::ReportTestConsts for Server {
            const FIRMWARE_FIRMWARE: FirmwareSummary<'static> = FirmwareSummary {
                name: MaxLen::new_unchecked("report_fw"),
                version: Version::full(1, 2, 3, Some("rc.1"), None),
                git: Some(GitInfo {
                    commit: MaxLen::new_unchecked(RefVec::new_bytes(&COMMIT)),
                    dirty: true,
                }),
                built: ww_firmware_info::DateTime::from_ymd_hms_utc_opt(2026, 10, 3, 18, 20, 0, 0),
                profile: BuildProfile::Release,
            };

            fn firmware_build_id(&self) -> MaxLen<RefVec<'_, u8>, 32> {
                MaxLen::new_unchecked(RefVec::new_bytes(&self.build_id))
            }

            const FIRMWARE_BUILD_RECORD: MaxLen<RefVec<'static, u8>, 512> =
                MaxLen::new_unchecked(RefVec::new_bytes(b"\xB1\xD0record"));

            fn board_board(&self) -> BoardSummary<'_> {
                BoardSummary {
                    name: MaxLen::new_unchecked("rePort"),
                    revision: MaxLen::new_unchecked("B"),
                    revision_source: RevisionSource::Strap { value: 2 },
                    variant: MaxLen::new_unchecked(""),
                    bom_variant: MaxLen::new_unchecked("std"),
                    serial: MaxLen::new_unchecked("RP-0042"),
                    build_date: None,
                }
            }

            const BOARD_REWORKS: MaxLen<RefVec<'static, ww_board_info::Rework<'static>>, 16> =
                MaxLen::new_unchecked(RefVec::Slice {
                    slice: &[
                        ww_board_info::Rework {
                            id: MaxLen::new_unchecked("rework-1"),
                            title: MaxLen::new_unchecked("0R to 10k on R12"),
                            applied: true,
                        },
                        ww_board_info::Rework {
                            id: MaxLen::new_unchecked("rework-3"),
                            title: MaxLen::new_unchecked("cut trace to U3 pin 4"),
                            applied: false,
                        },
                    ],
                });

            fn uid_ids(&self) -> MaxLen<RefVec<'_, UniqueId<'_>>, 4> {
                static VALUE: [u8; 8] = [0xe6, 0x61, 0x38, 0x71, 0x23, 0x5b, 0x1c, 0x2d];
                MaxLen::new_unchecked(RefVec::Slice {
                    slice: const {
                        &[UniqueId {
                            kind: UidKind::ChipUnique,
                            source: UidSource::InternalIc,
                            value: MaxLen::new_unchecked(RefVec::new_bytes(&VALUE)),
                        }]
                    },
                })
            }

            const USER_CAPABILITIES: UserInfoCaps = UserInfoCaps {
                name_max: 32,
                notes_max: 256,
                persistent: true,
            };

            fn defmt_defmt(&self) -> ww_log::DefmtInfo<'_> {
                ww_log::DefmtInfo {
                    encoding: ww_defmt_logger::encoding(),
                    build_id: MaxLen::new_unchecked(RefVec::new_bytes(&self.build_id)),
                }
            }

            fn counters_info(&self) -> CountersInfo<'_> {
                CountersInfo {
                    format: cnt_core::SIGNATURE_VERSION,
                    ram: BufferInfo::new(RAM_USED, &self.ram),
                    backup: Some(BufferInfo::new(BKP_USED, &self.bkp)),
                    build_id: MaxLen::new_unchecked(RefVec::new_bytes(&self.build_id)),
                }
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                device_report_api :: ReportTest for super::Server,
                server = true, no_alloc = true, use_async = true,
                method_model = "_=immediate",
                property_model = "_=get_set",
                const_model = "BUILD_ID=fn,IDS=fn,BOARD=fn,DEFMT=fn,INFO=fn",
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

            async fn poll(&mut self, out: &mut impl EventOut, now_ms: u64) {
                self.pump().await;
                let mut buf = [0u8; 1024];
                _ = self.poll_streams(out, now_ms, &mut buf, 4096).await;
            }
        }
    }

    /// Set the counters the report shows: boots = 3, loops = 2^32 + 5, uart/CrcError = 2, radio/RxBytes = 100.
    fn set_counters(server: &server::Server) {
        for (idx, v) in [(0, 3), (1, 5), (2, 1), (4, 2)] {
            server.ram[idx].store(v, Ordering::Relaxed);
        }
        server.bkp[2].store(100, Ordering::Relaxed);
    }

    async fn start(path: &str, build_id: Vec<u8>, logs: u32) -> (TestDevice, u32, String) {
        let _ = tracing_subscriber::fmt::try_init();
        let (pc, name) = some_function();
        let mut server = server::Server::boot(build_id, pc).await;
        set_counters(&server);
        // logged before any host connects
        for value in 0..logs {
            server.log_frame(value);
        }
        server.pump().await;
        let device = tests_common::start_device_async(
            path,
            server,
            device_report_api::REPORT_TEST_FULL_GID,
            server::api_impl::api_hash(),
        );
        (device, pc, name)
    }

    /// `ww info` connects traits-only, `ww report` also reads the device's introspection data (the log stream).
    async fn connect_traits_only(device: &TestDevice) -> DynClient {
        DynClient::traits_only(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    async fn connect(device: &TestDevice) -> DynClient {
        DynClient::from_config(ClientConfig::new().in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    fn defmt_table() -> Arc<defmt_decoder::Table> {
        Arc::new(table(match ww_defmt_logger::encoding() {
            ww_log::DefmtEncoding::Raw => "Raw",
            ww_log::DefmtEncoding::Rzcobs => "Rzcobs",
            e => panic!("{e:?}"),
        }))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn info_of_any_device() {
        let (device, _, _) = start("device_report/info", fixture_build_id(), 0).await;
        let client = connect_traits_only(&device).await;
        let info = collect_info(&Contract::new(&client)).await;

        let fw = info.firmware.value().expect("firmware");
        assert_eq!(fw.name, "report_fw");
        assert_eq!(fw.version, "1.2.3-rc.1");
        assert_eq!(fw.git.as_deref(), Some("a1a1a1a-dirty"));
        assert_eq!(fw.built.as_deref(), Some("2026-10-03 18:20"));
        assert_eq!(fw.profile, "release");
        assert_eq!(fw.build_id, ww_report::info::hex(&fixture_build_id()));
        assert_eq!(fw.build_record_bytes, Some(8));

        let board = info.board.value().expect("board");
        assert_eq!(board.name, "rePort");
        assert_eq!(board.revision, "B");
        assert_eq!(board.revision_source, "strap 2");
        assert_eq!(board.serial, "RP-0042");
        let reworks = board.reworks.value().expect("reworks");
        assert_eq!(reworks.len(), 2);
        assert_eq!(reworks[0].id, "rework-1");
        assert!(reworks[0].applied && !reworks[1].applied);
        let text = ww_report::report::render_info(&info);
        assert!(
            text.contains("Reworks    rework-1 0R to 10k on R12 (applied)"),
            "{text}"
        );
        assert!(
            text.contains("           rework-3 cut trace to U3 pin 4 (not applied)"),
            "{text}"
        );

        let ids = info.ids.value().expect("ids");
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0].kind, "chip");
        assert_eq!(ids[0].value, "e66138712 35b1c2d".replace(' ', ""));

        let label = info.label.value().expect("label");
        assert_eq!(label.name, "bench 2");
        assert_eq!(label.notes, Part::Value("DUT of the CAN test".to_string()));
        assert!(label.persistent);

        let reset = info.last_reset.value().expect("last reset");
        assert_eq!(reset.cause, "Panic");
        assert_eq!(reset.message, "index out of bounds");
        assert_eq!(reset.boot_count, Part::Value(3));
        assert_eq!(reset.uptime_s, Some(2.5));
        assert_eq!(reset.stack.len(), 3);
        assert_eq!(reset.boots_since_crash, 2);
    }

    /// Counters decoded with the ELF, the log from before the host connected decoded with the table, the crash's
    /// addresses named with the ELF.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn report_decodes_everything() {
        let (device, _, function) = start("device_report/report", fixture_build_id(), 5).await;
        let client = connect(&device).await;
        let report = collect_report(
            &Contract::new(&client),
            &ReportOptions {
                elf: Some(fixture()),
                defmt_table: Some(defmt_table()),
                tool: "ww test".to_string(),
                ..Default::default()
            },
        )
        .await;

        let counters = report.counters.value().expect("counters");
        let nonzero: Vec<_> = counters
            .counters
            .iter()
            .filter(|c| c.value != 0)
            .map(|c| (c.buffer.as_str(), c.name.as_str(), c.value))
            .collect();
        assert_eq!(
            nonzero,
            [
                ("RAM", "boots", 3),
                ("RAM", "loops", (1 << 32) + 5),
                ("RAM", "uart/CrcError", 2),
                ("BKP", "radio/RxBytes", 100),
            ]
        );

        let log = report
            .log
            .value()
            .unwrap_or_else(|| panic!("log: {:?}", report.log));
        assert_eq!(log.lost, 0);
        assert_eq!(log.lines.len(), 5, "{:?}", log.lines);
        for (value, line) in log.lines.iter().enumerate() {
            assert!(line.contains(&format!("sample {value}")), "{line}");
        }

        let symbols = report.reset_symbols.value().expect("symbols");
        // pc, lr, then the stack
        assert_eq!(symbols.len(), 5);
        assert_eq!(symbols[0].function.as_deref(), Some(function.as_str()));
        assert_eq!(symbols[0].offset, 4);
        // a thumb return address (bit 0 set) finds the same function
        assert_eq!(symbols[2].function.as_deref(), Some(function.as_str()));
        assert_eq!(symbols[3].function, None);

        let text = render_report(&report);
        assert!(text.contains("report_fw 1.2.3-rc.1"), "{text}");
        assert!(text.contains("boots"), "{text}");
        assert!(text.contains("sample 4"), "{text}");
        assert!(text.contains(&function), "{text}");
        assert!(text.contains("Cause: Panic"), "{text}");
        assert!(text.contains("Boots since the crash: 2"), "{text}");
        // the whole report is JSON-serializable for scripts
        serde_json::to_string(&report).unwrap();
    }

    /// Reading the log leaves the device's store alone: a second report has the same lines.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reading_the_log_twice() {
        let (device, _, _) = start("device_report/twice", fixture_build_id(), 3).await;
        let client = connect(&device).await;
        let options = ReportOptions {
            elf: Some(fixture()),
            defmt_table: Some(defmt_table()),
            ..Default::default()
        };
        let contract = Contract::new(&client);
        let first = collect_report(&contract, &options).await;
        let second = collect_report(&contract, &options).await;
        assert_eq!(first.log.value().unwrap().lines.len(), 3);
        assert_eq!(first.log, second.log);
    }

    /// No ELF anywhere: counters and log say why, info and the rest of the report are still there.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn elf_not_found_is_said() {
        let (device, _, _) = start("device_report/no_elf", vec![0x55; 20], 2).await;
        let client = connect(&device).await;
        let report = collect_report(&Contract::new(&client), &ReportOptions::default()).await;
        assert!(report.info.firmware.value().is_some());
        match &report.counters {
            Part::Error(e) => assert!(e.contains("No ELF with the device's build id"), "{e}"),
            other => panic!("{other:?}"),
        }
        assert!(matches!(report.log, Part::Error(_)));
        assert!(matches!(report.reset_symbols, Part::Error(_)));
        let text = render_report(&report);
        assert!(
            text.contains("unavailable: No ELF with the device's build id"),
            "{text}"
        );
        assert!(text.contains("report_fw"), "{text}");
    }

    /// An ELF of other firmware is refused, `--force` uses it with a note.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn elf_of_other_firmware() {
        let (device, _, _) = start("device_report/mismatch", vec![0x55; 20], 0).await;
        let client = connect(&device).await;
        let mut options = ReportOptions {
            elf: Some(fixture()),
            defmt_table: Some(defmt_table()),
            ..Default::default()
        };
        let contract = Contract::new(&client);
        let report = collect_report(&contract, &options).await;
        match &report.counters {
            Part::Error(e) => assert!(e.contains("wrong names"), "{e}"),
            other => panic!("{other:?}"),
        }
        options.force = true;
        let report = collect_report(&contract, &options).await;
        assert!(report.counters.value().is_some());
        assert!(
            report.notes.iter().any(|n| n.contains("--force")),
            "{:?}",
            report.notes
        );
    }
}

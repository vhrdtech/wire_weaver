//! `ww_block` and `ww_sd` end to end: a mock card of 64 blocks served as a block device and as a raw SD card on
//! `ww_device::Server` through `tests_common::start_device`, the generated client through the real host event loop.
//! The two traits share the card's write lease, as a bring-up firmware would.

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Instant;
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use ww_block::{
        BlockError, BlockErrorOwned, Erased, Geometry, MediumKind, ReadRun, RunAck, RunStatus,
        WriteRunOwned, crc32,
    };
    use ww_sd::{
        Access, BusState, BusWidth, CardKind, CardState, CardStatus, DataPhaseOwned, InitStep,
        Interface, Outcome, RawCommandOwned, ResponseType, SdError, SdErrorOwned, SpeedMode,
    };
    use ww_stream::tx::Counted;

    const BLOCK: usize = 512;
    const BLOCKS: u64 = 64;

    /// What the test watches: the card's own user (a recorder) and the lease.
    #[derive(Default)]
    struct Shared {
        /// The card's user runs (logs); stopped while a lease is held.
        user_running: bool,
        /// Raw commands the card received, `(app, index)`.
        commands: Vec<(bool, u8)>,
    }

    struct Lease {
        id: u32,
        lease_s: u16,
        last: Instant,
    }

    mod server {
        use super::*;
        use tests_common::TestProcessEvents;
        use ww_client_server::credit::SinkCredit;

        pub struct Server {
            pub m: Arc<Mutex<Shared>>,
            pub card: Vec<u8>,
            pub lease: Option<Lease>,
            pub next_lease: u32,
            /// Range of the open `read_runs` stream still to send.
            pub read_left: Option<(u64, u64)>,
            pub write_runs_credit: SinkCredit,
            pub written_stream: Counted,
            pub read_runs_stream: Counted,
            pub stream_sessions: u32,
            /// The last data phase read, borrowed by `sd_command`'s answer.
            pub sd_data: Vec<u8>,
            pub sd_response: [u8; 4],
        }

        fn range_ok(lba: u64, count: u64) -> bool {
            lba.checked_add(count).is_some_and(|end| end <= BLOCKS)
        }

        impl Server {
            /// Renews and checks the lease: `false` for a lapsed or other one.
            fn lease_ok(&mut self, lease: u32) -> bool {
                match &mut self.lease {
                    Some(l) if l.id == lease && l.last.elapsed().as_secs() < l.lease_s as u64 => {
                        l.last = Instant::now();
                        true
                    }
                    _ => false,
                }
            }

            fn write_blocks(&mut self, lease: u32, lba: u64, data: &[u8], crc: u32) -> RunStatus {
                if !data.len().is_multiple_of(BLOCK) || data.len() > BLOCK {
                    return RunStatus::BadLength;
                }
                if !range_ok(lba, (data.len() / BLOCK) as u64) {
                    return RunStatus::OutOfRange;
                }
                if !self.lease_ok(lease) {
                    return RunStatus::BadLease;
                }
                if crc32(data) != crc {
                    return RunStatus::BadCrc;
                }
                let at = lba as usize * BLOCK;
                self.card[at..at + data.len()].copy_from_slice(data);
                RunStatus::Written
            }

            /// The event loop's producer: the next run of an open `read_runs`, one block (`max_blocks`) per request,
            /// as one 1 KiB event buffer holds.
            fn produce(&mut self, out: &mut impl BlockingEventOut) {
                let Some((lba, count)) = self.read_left.take() else {
                    return;
                };
                let at = lba as usize * BLOCK;
                let run = ReadRun {
                    lba,
                    data: MaxLen::new(RefVec::new_bytes(&self.card[at..at + BLOCK])).unwrap(),
                };
                api_impl::stream_data_ser()
                    .card()
                    .read_runs_send_blocking(&mut self.read_runs_stream, &run, 1, None, out)
                    .unwrap();
                if count > 1 {
                    self.read_left = Some((lba + 1, count - 1));
                }
            }

            // ---- ww_block::BlockDevice at card ----

            fn card_geometry(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Result<Geometry<'_>, BlockError<'_>>> {
                Ready(Ok(Geometry {
                    kind: MediumKind::SdCard,
                    block_size: BLOCK as u32,
                    block_count: BLOCKS,
                    max_blocks: 1,
                    erase_blocks: 16,
                    best_write_blocks: None,
                    erased: Erased::Zeros,
                    erase_before_write: false,
                    read_only: false,
                    medium_id: 0x1234,
                    name: MaxLen::new_unchecked("MOCK1"),
                }))
            }

            fn card_read(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                lba: u64,
                count: u16,
            ) -> RpcResult<Result<MaxLen<RefVec<'_, u8>, 4096>, BlockError<'_>>> {
                if !range_ok(lba, count as u64) {
                    return Ready(Err(BlockError::OutOfRange));
                }
                if count > 1 {
                    return Ready(Err(CommonError::InvalidArgument {
                        arg: 1,
                        reason: MaxLen::new_unchecked("over max_blocks"),
                    }
                    .into()));
                }
                let at = lba as usize * BLOCK;
                let data = &self.card[at..at + count as usize * BLOCK];
                Ready(Ok(MaxLen::new(RefVec::new_bytes(data)).unwrap()))
            }

            fn card_crc(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                lba: u64,
                count: u32,
            ) -> RpcResult<Result<u32, BlockError<'_>>> {
                if !range_ok(lba, count as u64) {
                    return Ready(Err(BlockError::OutOfRange));
                }
                let at = lba as usize * BLOCK;
                Ready(Ok(crc32(&self.card[at..at + count as usize * BLOCK])))
            }

            fn card_claim(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                lease_s: u16,
            ) -> RpcResult<Result<u32, BlockError<'_>>> {
                if self
                    .lease
                    .as_ref()
                    .is_some_and(|l| l.last.elapsed().as_secs() < l.lease_s as u64)
                {
                    return Ready(Err(BlockError::busy()));
                }
                // stops the card's user before handing out the lease
                self.m.lock().unwrap().user_running = false;
                self.next_lease = self.next_lease.wrapping_add(1);
                self.lease = Some(Lease {
                    id: self.next_lease,
                    lease_s,
                    last: Instant::now(),
                });
                Ready(Ok(self.next_lease))
            }

            fn card_release(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                lease: u32,
            ) -> RpcResult<Result<(), BlockError<'_>>> {
                if !self.lease_ok(lease) {
                    return Ready(Err(BlockError::BadLease));
                }
                self.lease = None;
                self.m.lock().unwrap().user_running = true;
                Ready(Ok(()))
            }

            fn card_write(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                lease: u32,
                lba: u64,
                data: MaxLen<RefVec<'_, u8>, 4096>,
                crc: u32,
            ) -> RpcResult<Result<(), BlockError<'_>>> {
                let data = data.as_slice().to_vec();
                Ready(match self.write_blocks(lease, lba, &data, crc) {
                    RunStatus::Written => Ok(()),
                    RunStatus::OutOfRange => Err(BlockError::OutOfRange),
                    RunStatus::BadLease => Err(BlockError::BadLease),
                    RunStatus::BadCrc => Err(BlockError::BadCrc),
                    _ => Err(CommonError::InvalidArgument {
                        arg: 2,
                        reason: MaxLen::new_unchecked("not whole blocks"),
                    }
                    .into()),
                })
            }

            fn card_erase(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                lease: u32,
                lba: u64,
                count: u32,
            ) -> RpcResult<Result<(), BlockError<'_>>> {
                if !range_ok(lba, count as u64) {
                    return Ready(Err(BlockError::OutOfRange));
                }
                if !self.lease_ok(lease) {
                    return Ready(Err(BlockError::BadLease));
                }
                let at = lba as usize * BLOCK;
                self.card[at..at + count as usize * BLOCK].fill(0);
                Ready(Ok(()))
            }

            fn credit_card_write_runs(&mut self) -> &mut SinkCredit {
                &mut self.write_runs_credit
            }

            fn sideband_card_write_runs(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }

            fn write_card_write_runs(
                &mut self,
                cx: &mut Context<'_, impl BlockingEventOut>,
                value: ww_block::WriteRun<'_>,
            ) {
                let data = value.data.as_slice().to_vec();
                let status = self.write_blocks(value.lease, value.lba, &data, value.crc);
                let len = self.write_runs_credit.last_item_len();
                self.write_runs_credit.free(len);
                let ack = RunAck {
                    lba: value.lba,
                    count: (data.len() / BLOCK) as u32,
                    status,
                };
                let _ = api_impl::stream_data_ser().card().written_send_blocking(
                    &mut self.written_stream,
                    &ack,
                    1,
                    None,
                    cx,
                );
            }

            fn card_written(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<()> {
                Ready(())
            }

            fn close_card_written(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn stream_card_written(&mut self) -> &mut Counted {
                &mut self.written_stream
            }

            fn card_read_runs(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                lba: u64,
                count: u64,
            ) -> RpcResult<Result<(), BlockError<'_>>> {
                if !range_ok(lba, count) {
                    return Ready(Err(BlockError::OutOfRange));
                }
                self.read_left = Some((lba, count));
                Ready(Ok(()))
            }

            fn close_card_read_runs(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {
                self.read_left = None;
            }

            fn stream_card_read_runs(&mut self) -> &mut Counted {
                &mut self.read_runs_stream
            }

            fn new_stream_session(&mut self) -> u32 {
                self.stream_sessions = self.stream_sessions.wrapping_add(1);
                self.stream_sessions
            }

            // ---- ww_sd::SdCard at sd ----

            fn sd_capabilities(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<ww_sd::Capabilities> {
                Ready(ww_sd::Capabilities {
                    interface: Interface::Sd,
                    max_bus_width: BusWidth::Four,
                    max_clock_khz: 50_000,
                    uhs: false,
                    unrestricted: ww_sd::UNRESTRICTED,
                })
            }

            fn sd_state(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<CardState> {
                Ready(card_state(self.lease.is_some()))
            }

            fn sd_detect(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
                Ready(())
            }

            fn close_sd_detect(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn sideband_sd_detect(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }

            fn sd_registers(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Result<ww_sd::SdRegisters<'_>, SdError<'_>>> {
                let mut cid = [0u8; 16];
                cid[0] = 0x03; // MID
                cid[3..8].copy_from_slice(b"MOCK1");
                Ready(Ok(ww_sd::SdRegisters {
                    cid,
                    csd: [0x40; 16],
                    ocr: 0xc0ff_8000,
                    rca: 0xaaaa,
                    scr: Some([0x02, 0x35, 0x80, 0, 0, 0, 0, 0]),
                    ssr: None,
                }))
            }

            fn sd_reinit(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                lease: u32,
                _max_clock_khz: u32,
                _bus_width: BusWidth,
            ) -> RpcResult<Result<ww_sd::InitReport<'_>, SdError<'_>>> {
                if !self.lease_ok(lease) {
                    return Ready(Err(SdError::BadLease));
                }
                const STEPS: [InitStep; 3] = [
                    step(0, false, 0, 0, 1),
                    step(8, false, 0x1aa, 0x1aa, 1),
                    step(41, true, 0x5130_0000, 0xc0ff_8000, 12),
                ];
                Ready(Ok(ww_sd::InitReport {
                    steps: MaxLen::new(RefVec::Slice { slice: &STEPS }).unwrap(),
                    state: card_state(true),
                    total_us: 250_000,
                }))
            }

            fn sd_command(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                lease: Option<u32>,
                cmd: ww_sd::RawCommand<'_>,
            ) -> RpcResult<Result<ww_sd::RawResponse<'_>, SdError<'_>>> {
                let lease_ok = lease.is_some_and(|l| self.lease_ok(l));
                if let Err(e) = ww_sd::check_access(&cmd, lease_ok) {
                    return Ready(Err(e));
                }
                self.m.lock().unwrap().commands.push((cmd.app, cmd.index));
                // the mock answers CMD13 and CMD17, and times out on anything else
                let tran = CardStatus {
                    bits: (4 << 9) | CardStatus::READY_FOR_DATA,
                };
                let (outcome, status) = match (cmd.app, cmd.index) {
                    (false, 13) => (Outcome::Ok, Some(tran)),
                    (false, 17) => {
                        let at = cmd.arg as usize * BLOCK;
                        self.sd_data = self.card[at..at + BLOCK].to_vec();
                        (Outcome::Ok, Some(tran))
                    }
                    _ => (Outcome::ResponseTimeout, None),
                };
                if cmd.index != 17 {
                    self.sd_data.clear();
                }
                let response: &[u8] = match status {
                    Some(s) => {
                        self.sd_response = s.bits.to_be_bytes();
                        &self.sd_response
                    }
                    None => &[],
                };
                Ready(Ok(ww_sd::RawResponse {
                    outcome,
                    response: MaxLen::new(RefVec::new_bytes(response)).unwrap(),
                    status,
                    data: MaxLen::new(RefVec::new_bytes(&self.sd_data)).unwrap(),
                    timing: ww_sd::Timing {
                        response_us: status.map(|_| 3),
                        data_us: (cmd.index == 17).then_some(120),
                        busy_us: None,
                    },
                }))
            }
        }

        const fn step(index: u8, app: bool, arg: u32, response: u32, repeats: u16) -> InitStep {
            InitStep {
                index,
                app,
                arg,
                response,
                outcome: Outcome::Ok,
                repeats,
                us: 100 * repeats as u32,
            }
        }

        fn card_state(leased: bool) -> CardState {
            CardState {
                present: true,
                kind: Some(CardKind::Sdhc),
                bus: Some(BusState {
                    width: BusWidth::Four,
                    clock_khz: 25_000,
                    speed: SpeedMode::DefaultSpeed,
                    low_voltage: false,
                }),
                leased,
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                block_sd_api :: BlockSdTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                // debug_to_file = "../../target/tests_block_sd_server.rs"
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
                // the mock's producer runs between requests
                self.produce(out);
                self.process_request_bytes(bytes, scratch, out, medium)
            }
        }
    }

    mod client {
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
                block_sd_api :: BlockSdTest for super::StdClient,
                client = "std_client",
                // debug_to_file = "../../target/tests_block_sd_client.rs"
            );
        }
    }

    use client::StdClient;

    fn start(path: &str) -> (TestDevice, Arc<Mutex<Shared>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let m = Arc::new(Mutex::new(Shared {
            user_running: true,
            ..Default::default()
        }));
        let server = server::Server {
            m: m.clone(),
            // block i holds the byte i
            card: (0..BLOCKS as usize * BLOCK)
                .map(|i| (i / BLOCK) as u8)
                .collect(),
            lease: None,
            next_lease: 0,
            read_left: None,
            write_runs_credit: ww_client_server::credit::SinkCredit::new(
                2 * ww_block::MAX_TRANSFER as u32 + 64,
            ),
            written_stream: Counted::new(),
            read_runs_stream: Counted::new(),
            stream_sessions: 0,
            sd_data: vec![],
            sd_response: [0; 4],
        };
        let device = tests_common::start_device(
            path,
            server,
            block_sd_api::BLOCK_SD_TEST_FULL_GID,
            server::api_impl::api_hash(),
        );
        (device, m)
    }

    async fn connect(device: &TestDevice) -> StdClient {
        StdClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    fn block_error(
        r: Result<impl std::fmt::Debug, wire_weaver_client::CallError<BlockErrorOwned>>,
    ) -> BlockErrorOwned {
        r.unwrap_err().device().cloned().expect("a device error")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn block_read_crc_and_leased_write() {
        let (device, m) = start("block_sd/block");
        let client = connect(&device).await;

        let g = client.card().geometry().call().await.unwrap();
        assert_eq!((g.block_size, g.block_count), (512, BLOCKS));
        assert_eq!(g.name.as_str(), "MOCK1");

        // reads are free
        let mut data = client.card().read(3, 1).call().await.unwrap().into_inner();
        data.extend(client.card().read(4, 1).call().await.unwrap().into_inner());
        assert!(data[..BLOCK].iter().all(|&b| b == 3) && data[BLOCK..].iter().all(|&b| b == 4));
        // the CRC over both blocks, computed on the device
        let crc = client.card().crc(3, 2).call().await.unwrap();
        assert_eq!(crc, crc32(&data));
        assert!(matches!(
            block_error(client.card().read(3, 2).call().await),
            BlockErrorOwned::Common(CommonErrorOwned::InvalidArgument { .. })
        ));
        assert_eq!(
            block_error(client.card().read(64, 1).call().await),
            BlockErrorOwned::OutOfRange
        );

        // writes need the lease
        let new = vec![0xa5u8; BLOCK];
        let r = client
            .card()
            .write(1, 10, MaxLen::new(new.clone()).unwrap(), crc32(&new))
            .call()
            .await;
        assert_eq!(block_error(r), BlockErrorOwned::BadLease);

        // claiming stops the card's user, one lease at a time
        assert!(m.lock().unwrap().user_running);
        let lease = client.card().claim(10).call().await.unwrap();
        assert!(!m.lock().unwrap().user_running);
        assert!(matches!(
            block_error(client.card().claim(10).call().await),
            BlockErrorOwned::Common(CommonErrorOwned::Busy)
        ));

        // a wrong CRC writes nothing
        let r = client
            .card()
            .write(
                lease,
                10,
                MaxLen::new(new.clone()).unwrap(),
                crc32(&new) ^ 1,
            )
            .call()
            .await;
        assert_eq!(block_error(r), BlockErrorOwned::BadCrc);
        assert!(
            client
                .card()
                .read(10, 1)
                .call()
                .await
                .unwrap()
                .iter()
                .all(|&b| b == 10)
        );

        client
            .card()
            .write(lease, 10, MaxLen::new(new.clone()).unwrap(), crc32(&new))
            .call()
            .await
            .unwrap();
        assert_eq!(client.card().crc(10, 1).call().await.unwrap(), crc32(&new));

        client.card().erase(lease, 10, 1).call().await.unwrap();
        assert!(
            client
                .card()
                .read(10, 1)
                .call()
                .await
                .unwrap()
                .iter()
                .all(|&b| b == 0)
        );

        client.card().release(lease).call().await.unwrap();
        assert!(m.lock().unwrap().user_running);
        assert_eq!(
            block_error(client.card().release(lease).call().await),
            BlockErrorOwned::BadLease
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn block_bulk_write_and_read() {
        let (device, _m) = start("block_sd/bulk");
        let client = connect(&device).await;
        let lease = client.card().claim(10).call().await.unwrap();

        let mut written = client.card().written().call().await.unwrap();
        let mut runs = client.card().write_runs().await.unwrap();
        let run = |lba: u64, byte: u8, crc_ok: bool| {
            let data = vec![byte; BLOCK];
            let crc = crc32(&data) ^ (!crc_ok as u32);
            WriteRunOwned {
                lease,
                lba,
                crc,
                data: MaxLen::new(data).unwrap(),
            }
        };
        runs.send_owned(run(20, 0x11, true)).await.unwrap();
        runs.send_owned(run(21, 0x22, false)).await.unwrap();
        runs.send_owned(run(64, 0x33, true)).await.unwrap();
        runs.flush().await.unwrap();
        for (lba, status) in [
            (20, RunStatus::Written),
            (21, RunStatus::BadCrc),
            (64, RunStatus::OutOfRange),
        ] {
            let ack = written.recv().await.unwrap();
            assert_eq!((ack.lba, ack.count, ack.status), (lba, 1, status));
        }

        // read 3 blocks back as runs on the stream, one block each
        let (_, mut read) = client
            .card()
            .read_runs(20, 3)
            .call()
            .await
            .unwrap()
            .unwrap();
        let mut got = vec![];
        while got.len() < 3 * BLOCK {
            // any request lets the mock's event loop send the next run
            client.card().geometry().call().await.unwrap();
            let ReadRunOwned { lba, data } = read.recv().await.unwrap();
            assert_eq!(lba, 20 + (got.len() / BLOCK) as u64);
            got.extend_from_slice(&data);
        }
        assert!(got[..BLOCK].iter().all(|&b| b == 0x11));
        // the run with the bad CRC was not written
        assert!(got[BLOCK..2 * BLOCK].iter().all(|&b| b == 21));
        assert!(got[2 * BLOCK..].iter().all(|&b| b == 22));
        client.card().release(lease).call().await.unwrap();
    }

    use ww_block::ReadRunOwned;

    fn raw(index: u8, arg: u32, data: DataPhaseOwned) -> RawCommandOwned {
        RawCommandOwned {
            index,
            arg,
            app: false,
            response: ResponseType::R1,
            data,
            timeout_ms: None,
        }
    }

    fn sd_error(
        r: Result<impl std::fmt::Debug, wire_weaver_client::CallError<SdErrorOwned>>,
    ) -> SdErrorOwned {
        r.unwrap_err().device().cloned().expect("a device error")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sd_registers_reinit_and_raw_commands() {
        let (device, m) = start("block_sd/sd");
        let client = connect(&device).await;

        let caps = client.sd().capabilities().call().await.unwrap();
        assert_eq!(caps.unrestricted, ww_sd::UNRESTRICTED);
        let state = client.sd().state().call().await.unwrap();
        assert_eq!(state.kind, Some(CardKind::Sdhc));
        assert!(!state.leased);

        let regs = client.sd().registers().call().await.unwrap();
        assert_eq!(&regs.cid[3..8], b"MOCK1");
        assert_eq!(regs.scr, Some([0x02, 0x35, 0x80, 0, 0, 0, 0, 0]));

        // a free command: status, then a block read with its data and timing
        let r = client
            .sd()
            .command(None, raw(13, 0xaaaa << 16, DataPhaseOwned::None))
            .call()
            .await
            .unwrap();
        assert_eq!(r.outcome, Outcome::Ok);
        assert_eq!(r.status.unwrap().current_state(), 4);
        assert_eq!(r.response.len(), 4);
        let r = client
            .sd()
            .command(
                None,
                raw(
                    17,
                    7,
                    DataPhaseOwned::Read {
                        block_len: 512,
                        blocks: 1,
                    },
                ),
            )
            .call()
            .await
            .unwrap();
        assert!(r.data.iter().all(|&b| b == 7) && r.data.len() == BLOCK);
        assert_eq!(r.timing.data_us, Some(120));

        // erase and reinit need the lease of `ww_block`
        let erase = raw(38, 0, DataPhaseOwned::None);
        assert_eq!(erase.access(), Access::Lease);
        assert_eq!(
            sd_error(client.sd().command(None, erase.clone()).call().await),
            SdErrorOwned::BadLease
        );
        assert_eq!(
            sd_error(client.sd().reinit(1, 25_000, BusWidth::Four).call().await),
            SdErrorOwned::BadLease
        );
        let lease = client.card().claim(10).call().await.unwrap();
        assert!(client.sd().state().call().await.unwrap().leased);
        let r = client
            .sd()
            .command(Some(lease), erase)
            .call()
            .await
            .unwrap();
        // the mock's card doesn't answer it: not an error of the call, the outcome says so
        assert_eq!(r.outcome, Outcome::ResponseTimeout);
        assert!(r.status.is_none() && r.response.is_empty());
        let report = client
            .sd()
            .reinit(lease, 25_000, BusWidth::Four)
            .call()
            .await
            .unwrap();
        assert_eq!(report.steps.len(), 3);
        assert_eq!((report.steps[2].index, report.steps[2].repeats), (41, 12));

        let sent = m.lock().unwrap().commands.clone();
        assert_eq!(sent, [(false, 13), (false, 17), (false, 38)]);

        // CMD42 (lock/unlock) is refused even with the lease, unless built unrestricted
        let lock = raw(
            42,
            0,
            DataPhaseOwned::Write {
                block_len: 1,
                data: MaxLen::new(vec![0x08]).unwrap(),
            },
        );
        let r = client.sd().command(Some(lease), lock).call().await;
        if !ww_sd::UNRESTRICTED {
            assert_eq!(sd_error(r), SdErrorOwned::Restricted { index: 42 });
        }
        client.card().release(lease).call().await.unwrap();
    }
}

/// A small firmware serves `BlockDevice` without its `bulk` group: no sink, streams or handlers for it, and a host
/// calling them gets `Unimplemented`.
#[cfg(test)]
mod lean {
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use ww_block::{BlockError, Erased, Geometry, MediumKind};

    mod server {
        use super::*;
        use tests_common::TestProcessEvents;

        pub struct Server {
            pub card: [u8; 512],
        }

        impl Server {
            fn card_geometry(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Result<Geometry<'_>, BlockError<'_>>> {
                Ready(Ok(Geometry {
                    kind: MediumKind::NorFlash,
                    block_size: 512,
                    block_count: 1,
                    max_blocks: 1,
                    erase_blocks: 8,
                    best_write_blocks: None,
                    erased: Erased::Ones,
                    erase_before_write: true,
                    read_only: true,
                    medium_id: 0,
                    name: MaxLen::new_unchecked(""),
                }))
            }

            fn card_read(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                lba: u64,
                count: u16,
            ) -> RpcResult<Result<MaxLen<RefVec<'_, u8>, 4096>, BlockError<'_>>> {
                if lba != 0 || count != 1 {
                    return Ready(Err(BlockError::OutOfRange));
                }
                Ready(Ok(MaxLen::new(RefVec::new_bytes(&self.card)).unwrap()))
            }

            fn card_crc(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _lba: u64,
                _count: u32,
            ) -> RpcResult<Result<u32, BlockError<'_>>> {
                Ready(Ok(ww_block::crc32(&self.card)))
            }

            fn card_claim(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _lease_s: u16,
            ) -> RpcResult<Result<u32, BlockError<'_>>> {
                Ready(Err(BlockError::ReadOnly))
            }

            fn card_release(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _lease: u32,
            ) -> RpcResult<Result<(), BlockError<'_>>> {
                Ready(Err(BlockError::BadLease))
            }

            fn card_write(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _lease: u32,
                _lba: u64,
                _data: MaxLen<RefVec<'_, u8>, 4096>,
                _crc: u32,
            ) -> RpcResult<Result<(), BlockError<'_>>> {
                Ready(Err(BlockError::BadLease))
            }

            fn card_erase(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _lease: u32,
                _lba: u64,
                _count: u32,
            ) -> RpcResult<Result<(), BlockError<'_>>> {
                Ready(Err(BlockError::BadLease))
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                block_sd_api :: BlockLeanTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                omit = ["card.bulk"],
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

    mod client {
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
            // a host built against the whole API, not knowing what this device left out
            wire_weaver::ww_codegen!(
                block_sd_api :: BlockLeanTest for super::StdClient,
                client = "std_client",
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bulk_left_out() {
        let device: TestDevice = tests_common::start_device(
            "block_sd/lean",
            server::Server { card: [0xff; 512] },
            block_sd_api::BLOCK_LEAN_TEST_FULL_GID,
            server::api_impl::api_hash(),
        );
        let client = client::StdClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");

        let g = client.card().geometry().call().await.unwrap();
        assert!(g.read_only && g.erase_before_write);
        let data = client.card().read(0, 1).call().await.unwrap();
        assert_eq!(
            client.card().crc(0, 1).call().await.unwrap(),
            ww_block::crc32(&data)
        );
        assert!(matches!(
            client.card().claim(5).call().await.unwrap_err().device(),
            Some(ww_block::BlockErrorOwned::ReadOnly)
        ));

        // the bulk group isn't there: the device's introspection says so, the call fails without a round trip
        match client.card().read_runs(0, 1).call().await {
            Err(e) => assert!(e.is_unimplemented(), "{e:?}"),
            Ok(_) => panic!("read_runs is left out"),
        }
    }
}

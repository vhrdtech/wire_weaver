//! `ww_spi` end to end: a mock server on `ww_device::Server` through `tests_common::start_device`, the generated
//! client through the real host event loop.

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver_client::StreamEvent;
    use ww_numeric::NumericValue;
    use ww_si::{Prefix, Second, Volt};
    use ww_spi::{
        BitOrder, CsMode, SpiCapabilities, SpiConfig, SpiError, SpiErrorOwned, SpiFrame, SpiMode,
        SpiMonitorConfig, SpiOp, SpiRole, SpiSlaveAnswers, SpiSlaveConfig,
    };
    use ww_stream::tx::Counted;

    /// Smaller than [ww_spi::SPI_MAX], so a too-long transfer can be tested without building a 512-byte buffer.
    const MAX_LEN: usize = 64;

    fn volt(value: f32) -> Volt {
        Volt::from_parts(Prefix::Unit, NumericValue::F32(value))
    }

    fn second(prefix: Prefix, value: u32) -> Second {
        Second::from_parts(prefix, NumericValue::U32(value))
    }

    fn wrong_state() -> SpiError<'static> {
        CommonError::WrongState {
            reason: MaxLen::new_unchecked("not a master"),
        }
        .into()
    }

    fn leak_bytes(data: Vec<u8>) -> RefVec<'static, u8> {
        RefVec::Slice {
            slice: Box::leak(data.into_boxed_slice()),
        }
    }

    /// Bus state and what the mock bus received, shared with the test.
    #[derive(Default)]
    struct Model {
        configured: Option<SpiConfig>,
        cs_active: bool,
        /// Bytes of every `Spi::write`, in order.
        writes: Vec<Vec<u8>>,
        /// Set while the `frames` stream is open.
        monitor_config: Option<SpiMonitorConfig>,
        /// Frames the test makes the mock bus see: mosi, miso, CS asserted and released in µs.
        seen: Vec<(Vec<u8>, Vec<u8>, u32, u32)>,
    }

    mod no_std_sync_server {
        use super::*;
        use tests_common::TestProcessEvents;

        pub struct NoStdSyncServer {
            pub data: Arc<Mutex<Model>>,
            pub frames: Counted,
            pub stream_sessions: u32,
        }

        impl NoStdSyncServer {
            /// The event loop's producer: frames the monitor saw while `frames` is open.
            fn produce(&mut self, out: &mut impl BlockingEventOut) {
                let seen = std::mem::take(&mut self.data.lock().unwrap().seen);
                for (mosi, miso, start_us, end_us) in seen {
                    let frame = SpiFrame {
                        end: second(Prefix::Micro, end_us),
                        truncated: false,
                        mosi: MaxLen::new(RefVec::Slice { slice: &mosi }).unwrap(),
                        miso: MaxLen::new(RefVec::Slice { slice: &miso }).unwrap(),
                    };
                    let _ = api_impl::stream_data_ser().spi().monitor_send_blocking(
                        &mut self.frames,
                        &frame,
                        1,
                        Some(second(Prefix::Micro, start_us)),
                        out,
                    );
                }
            }

            fn new_stream_session(&mut self) -> u32 {
                self.stream_sessions = self.stream_sessions.wrapping_add(1);
                self.stream_sessions
            }
        }

        impl NoStdSyncServer {
            fn spi_transfer(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                data: MaxLen<RefVec<'_, u8>, { ww_spi::SPI_MAX }>,
            ) -> RpcResult<Result<MaxLen<RefVec<'_, u8>, { ww_spi::SPI_MAX }>, SpiError<'_>>>
            {
                let master = matches!(
                    self.data.lock().unwrap().configured.map(|c| c.role),
                    Some(SpiRole::Master)
                );
                if !master {
                    return Ready(Err(wrong_state()));
                }
                let bytes = data.into_inner().as_slice().to_vec();
                if bytes.len() > MAX_LEN {
                    return Ready(Err(SpiError::BufferTooBig));
                }
                // loopback: the mock peripheral echoes back what it was sent
                Ready(Ok(MaxLen::new(leak_bytes(bytes)).unwrap()))
            }

            fn spi_transaction(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                ops: MaxLen<RefVec<'_, SpiOp<'_>>, { ww_spi::MAX_OPS }>,
            ) -> RpcResult<Result<MaxLen<RefVec<'_, u8>, { ww_spi::SPI_MAX }>, SpiError<'_>>>
            {
                let master = matches!(
                    self.data.lock().unwrap().configured.map(|c| c.role),
                    Some(SpiRole::Master)
                );
                if !master {
                    return Ready(Err(wrong_state()));
                }
                let mut read_bytes = Vec::new();
                for op in ops.into_inner().iter() {
                    match op.unwrap() {
                        SpiOp::Write(_) => {}
                        // the mock peripheral has nothing to say: reads come back as 0xA5
                        SpiOp::Read(len) => {
                            read_bytes.extend(std::iter::repeat_n(0xA5u8, len as usize))
                        }
                        SpiOp::Transfer(data) => {
                            read_bytes.extend_from_slice(data.into_inner().as_slice())
                        }
                        SpiOp::Delay(_) => {}
                        // an op of a newer API version
                        SpiOp::Unknown(_) => return Ready(Err(CommonError::Unsupported.into())),
                    }
                }
                Ready(Ok(MaxLen::new(leak_bytes(read_bytes)).unwrap()))
            }

            fn spi_write(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                data: MaxLen<RefVec<'_, u8>, { ww_spi::SPI_MAX }>,
            ) -> RpcResult<Result<(), SpiError<'_>>> {
                let master = matches!(
                    self.data.lock().unwrap().configured.map(|c| c.role),
                    Some(SpiRole::Master)
                );
                if !master {
                    return Ready(Err(wrong_state()));
                }
                let bytes = data.into_inner().as_slice().to_vec();
                self.data.lock().unwrap().writes.push(bytes);
                Ready(Ok(()))
            }

            fn spi_capabilities(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<SpiCapabilities<'_>> {
                const IO_LEVELS: [Volt; 1] =
                    [Volt::from_parts(Prefix::Unit, NumericValue::F32(3.3))];
                Ready(SpiCapabilities {
                    min_freq: ww_si::Hertz::from_parts(Prefix::Kilo, NumericValue::U32(100)),
                    max_freq: ww_si::Hertz::from_parts(Prefix::Mega, NumericValue::U32(10)),
                    min_word_bits: 8,
                    max_word_bits: 8,
                    master: true,
                    io_levels: MaxLen::new(RefVec::Slice { slice: &IO_LEVELS }).unwrap(),
                    io_level_min: None,
                    io_level_max: None,
                    max_len: MAX_LEN as u32,
                })
            }

            fn spi_configure(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                config: SpiConfig,
            ) -> RpcResult<Result<SpiConfig, SpiError<'_>>> {
                if config.word_bits != 8 {
                    return Ready(Err(CommonError::InvalidArgument {
                        arg: 0,
                        reason: MaxLen::new_unchecked("word_bits"),
                    }
                    .into()));
                }
                // this mock's only IO level is 3.3 V, and that is what it runs at
                let io_level = match config.io_level {
                    None => volt(3.3),
                    Some(level) if level == volt(3.3) => level,
                    Some(_) => return Ready(Err(CommonError::Unsupported.into())),
                };
                let actual = SpiConfig {
                    io_level: Some(io_level),
                    ..config
                };
                self.data.lock().unwrap().configured = Some(actual);
                Ready(Ok(actual))
            }

            fn spi_set_cs(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                active: bool,
            ) -> RpcResult<Result<(), SpiError<'_>>> {
                let mut model = self.data.lock().unwrap();
                let manual = matches!(model.configured.map(|c| c.cs), Some(CsMode::Manual));
                if !manual {
                    return Ready(Err(wrong_state()));
                }
                model.cs_active = active;
                Ready(Ok(()))
            }

            fn spi_monitor(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                config: SpiMonitorConfig,
            ) -> RpcResult<Result<(), SpiError<'_>>> {
                if config.word_bits != 8 {
                    return Ready(Err(CommonError::Unsupported.into()));
                }
                self.data.lock().unwrap().monitor_config = Some(config);
                Ready(Ok(()))
            }

            fn close_spi_monitor(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {
                self.data.lock().unwrap().monitor_config = None;
            }

            fn stream_spi_monitor(&mut self) -> &mut Counted {
                &mut self.frames
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                io_spi_api :: IoSpiTest for super::NoStdSyncServer,
                server = true, no_alloc = true, use_async = false,
                omit = ["spi.slave"],
                method_model = "_=immediate",
                property_model = "_=get_set",
                // debug_to_file = "../../target/tests_io_spi_server.rs"
            );
        }

        impl TestProcessEvents for NoStdSyncServer {
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
                io_spi_api :: IoSpiTest for super::StdClient,
                client = "std_client",
                // debug_to_file = "../../target/tests_io_spi_client.rs"
            );
        }
    }

    use std_client::StdClient;

    fn start(path: &str) -> (TestDevice, Arc<Mutex<Model>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let data = Arc::new(Mutex::new(Model::default()));
        let server = no_std_sync_server::NoStdSyncServer {
            data: data.clone(),
            frames: Counted::new(),
            stream_sessions: 0,
        };
        let device = tests_common::start_device(
            path,
            server,
            io_spi_api::IO_SPI_TEST_FULL_GID,
            no_std_sync_server::api_impl::api_hash(),
        );
        (device, data)
    }

    async fn connect(device: &TestDevice) -> StdClient {
        StdClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    /// Wait for the device thread to process what was sent to it.
    async fn eventually(data: &Arc<Mutex<Model>>, f: impl Fn(&Model) -> bool) {
        for _ in 0..500 {
            if f(&data.lock().unwrap()) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        panic!("condition not met within 500ms");
    }

    fn configured_master(cs: CsMode) -> SpiConfig {
        SpiConfig {
            role: SpiRole::Master,
            freq: ww_si::Hertz::from_parts(Prefix::Mega, NumericValue::U32(1)),
            mode: SpiMode::Mode0,
            bit_order: BitOrder::MsbFirst,
            word_bits: 8,
            cs,
            io_level: None,
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn capabilities_and_configure() {
        let (device, _data) = start("io_spi/capabilities_and_configure");
        let client = connect(&device).await;

        let caps = client.spi().capabilities().call().await.unwrap();
        assert!(caps.master);
        assert_eq!(caps.max_len, MAX_LEN as u32);

        let actual = client
            .spi()
            .configure(configured_master(CsMode::Auto))
            .call()
            .await
            .unwrap();
        assert_eq!(actual.freq.value, NumericValue::U32(1));
        assert_eq!(actual.freq.prefix, Prefix::Mega);

        // this mock has no slave mode: the `slave` group is left out, the call fails without a round trip
        let slave = SpiSlaveConfig {
            mode: SpiMode::Mode0,
            bit_order: BitOrder::MsbFirst,
            word_bits: 8,
            io_level: None,
            answers: SpiSlaveAnswers::Queue {
                idle: 0xFF,
                repeat_last: false,
            },
        };
        match client.spi().slave_frames(slave).call().await {
            Err(e) => assert!(e.is_unimplemented(), "{e:?}"),
            Ok(_) => panic!("slave_frames is left out"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn transfer_write_and_size_limit() {
        let (device, data) = start("io_spi/transfer_write_and_size_limit");
        let client = connect(&device).await;
        client
            .spi()
            .configure(configured_master(CsMode::Auto))
            .call()
            .await
            .unwrap();

        let echoed = client
            .spi()
            .transfer(MaxLen::new(vec![1u8, 2, 3]).unwrap())
            .call()
            .await
            .unwrap();
        assert_eq!(echoed.into_inner(), vec![1, 2, 3]);

        client
            .spi()
            .write(MaxLen::new(vec![9u8, 8]).unwrap())
            .call()
            .await
            .unwrap();
        eventually(&data, |d| !d.writes.is_empty()).await;
        assert_eq!(data.lock().unwrap().writes, vec![vec![9u8, 8]]);

        let too_long = vec![0u8; MAX_LEN + 1];
        let err = client
            .spi()
            .transfer(MaxLen::new(too_long).unwrap())
            .call()
            .await;
        assert_eq!(
            err.unwrap_err().device(),
            Some(&SpiErrorOwned::BufferTooBig)
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn transaction_concatenates_reads_and_transfers() {
        let (device, _data) = start("io_spi/transaction");
        let client = connect(&device).await;
        client
            .spi()
            .configure(configured_master(CsMode::Auto))
            .call()
            .await
            .unwrap();

        let ops = vec![
            ww_spi::SpiOpOwned::Write(MaxLen::new(vec![1u8, 2]).unwrap()),
            ww_spi::SpiOpOwned::Read(2),
            ww_spi::SpiOpOwned::Transfer(MaxLen::new(vec![5u8, 6]).unwrap()),
            ww_spi::SpiOpOwned::Delay(second(Prefix::Milli, 1)),
        ];
        let read = client
            .spi()
            .transaction(MaxLen::new(ops).unwrap())
            .call()
            .await
            .unwrap();
        // the Write and Delay ops contribute no bytes; Read comes back as 0xA5, Transfer echoes
        assert_eq!(read.into_inner(), vec![0xA5, 0xA5, 5, 6]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn set_cs_requires_manual_mode() {
        let (device, data) = start("io_spi/set_cs");
        let client = connect(&device).await;
        client
            .spi()
            .configure(configured_master(CsMode::Auto))
            .call()
            .await
            .unwrap();

        let err = client.spi().set_cs(true).call().await;
        assert!(matches!(
            err.unwrap_err().device(),
            Some(SpiErrorOwned::Common(CommonErrorOwned::WrongState { .. }))
        ));

        client
            .spi()
            .configure(configured_master(CsMode::Manual))
            .call()
            .await
            .unwrap();
        client.spi().set_cs(true).call().await.unwrap();
        assert!(data.lock().unwrap().cs_active);
        client.spi().set_cs(false).call().await.unwrap();
        assert!(!data.lock().unwrap().cs_active);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn io_level_in_the_config() {
        let (device, data) = start("io_spi/io_level");
        let client = connect(&device).await;

        // `None` keeps the level, the answer says which one is in effect
        let actual = client
            .spi()
            .configure(configured_master(CsMode::Auto))
            .call()
            .await
            .unwrap();
        assert_eq!(actual.io_level, Some(volt(3.3)));

        let at_5v = SpiConfig {
            io_level: Some(volt(5.0)),
            ..configured_master(CsMode::Auto)
        };
        let err = client.spi().configure(at_5v).call().await;
        assert!(matches!(
            err.unwrap_err().device(),
            Some(SpiErrorOwned::Common(CommonErrorOwned::Unsupported))
        ));
        let configured = data.lock().unwrap().configured.unwrap();
        assert_eq!(configured.io_level, Some(volt(3.3)));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn monitor_frames_stream() {
        let (device, data) = start("io_spi/monitor");
        let client = connect(&device).await;
        let config = |word_bits| SpiMonitorConfig {
            mode: SpiMode::Mode0,
            bit_order: BitOrder::MsbFirst,
            word_bits,
            io_level: None,
        };
        let r = client.spi().monitor(config(9)).call().await.unwrap();
        assert!(
            matches!(r, Err(SpiErrorOwned::Common(CommonErrorOwned::Unsupported))),
            "{:?}",
            r.err()
        );

        let ((), mut stream) = client
            .spi()
            .monitor(config(8))
            .call()
            .await
            .unwrap()
            .unwrap();
        assert!(data.lock().unwrap().monitor_config.is_some());

        data.lock()
            .unwrap()
            .seen
            .push((vec![0x9F], vec![0x00, 0x00, 0x40], 100, 140));
        // any request lets the mock's event loop send it
        client.spi().capabilities().call().await.unwrap();
        match stream.recv_event().await.unwrap() {
            StreamEvent::Item { pos, t, value, .. } => {
                assert_eq!(pos, 0);
                // the head's U32 time comes unwrapped, as a U64
                assert_eq!(t.unwrap().value, NumericValue::U64(100));
                assert_eq!(value.end.value, NumericValue::U32(140));
                assert_eq!(value.mosi.into_inner(), vec![0x9F]);
                assert_eq!(value.miso.into_inner(), vec![0x00, 0x00, 0x40]);
                assert!(!value.truncated);
            }
            ev => panic!("{ev:?}"),
        }

        stream.close().await.unwrap();
        eventually(&data, |d| d.monitor_config.is_none()).await;
    }

    // ---- slave mode: a second mock server built on `ww_bus_slave::SpiSlave`, driven by `SpiBusOp` instead of a
    // real bus, with a fake clock advanced by `SpiBusOp::Advance` (STD-35, SRV-26) ----

    mod slave_mock {
        use super::*;
        use io_spi_api::{SpiBusOp, SpiBusResult};
        use tests_common::TestProcessEvents;
        use ww_bus_slave::SpiSlave;
        use ww_client_server::StreamSideband;
        use ww_client_server::credit::SinkCredit;
        use ww_spi::{SpiSlaveCapabilities, SpiSlaveConfig};

        /// Same sizes as `ww_bus_slave`'s own host tests: a 64-byte map, 3 answers of up to 8 bytes, 8 frames of up
        /// to 16 bytes each way.
        pub const MAP: usize = 64;
        const STAGE: usize = 32;
        pub const Q: usize = 3;
        const A: usize = 8;
        const EV: usize = 8;
        const REC: usize = 16;

        fn wrong_state() -> SpiError<'static> {
            CommonError::WrongState {
                reason: MaxLen::new_unchecked("slave-only mock"),
            }
            .into()
        }

        pub struct Server {
            engine: SpiSlave<MAP, STAGE, Q, A, EV, REC>,
            now_us: u32,
            frames: Counted,
            slave_answers_credit: SinkCredit,
            map_scratch: [u8; ww_spi::SPI_MAX],
            stream_sessions: u32,
        }

        impl Server {
            pub fn new() -> Self {
                Server {
                    engine: SpiSlave::new(),
                    now_us: 0,
                    frames: Counted::new(),
                    slave_answers_credit: SinkCredit::new(1024),
                    map_scratch: [0u8; ww_spi::SPI_MAX],
                    stream_sessions: 0,
                }
            }

            /// Drains the engine's frames (and drop count) onto `slave_frames`, see
            /// `IoSpiSlaveTest::spi_slave_pump`.
            fn pump(&mut self, out: &mut impl BlockingEventOut) {
                // drain what the ring kept first: a drop (reported after) always follows the oldest kept record,
                // the ring only drops once it is full of them.
                while let Some((t, f)) = self.engine.peek_event() {
                    let _ = api_impl::stream_data_ser()
                        .spi()
                        .slave_frames_send_blocking(&mut self.frames, &f, 1, Some(t), out);
                    self.engine.pop_event();
                }
                let dropped = self.engine.take_dropped();
                if dropped > 0 {
                    self.frames.lost(dropped as u64);
                }
            }

            fn new_stream_session(&mut self) -> u32 {
                self.stream_sessions = self.stream_sessions.wrapping_add(1);
                self.stream_sessions
            }

            fn spi_transfer(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _data: MaxLen<RefVec<'_, u8>, { ww_spi::SPI_MAX }>,
            ) -> RpcResult<Result<MaxLen<RefVec<'_, u8>, { ww_spi::SPI_MAX }>, SpiError<'_>>>
            {
                Ready(Err(wrong_state()))
            }

            fn spi_transaction(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _ops: MaxLen<RefVec<'_, SpiOp<'_>>, { ww_spi::MAX_OPS }>,
            ) -> RpcResult<Result<MaxLen<RefVec<'_, u8>, { ww_spi::SPI_MAX }>, SpiError<'_>>>
            {
                Ready(Err(wrong_state()))
            }

            fn spi_write(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _data: MaxLen<RefVec<'_, u8>, { ww_spi::SPI_MAX }>,
            ) -> RpcResult<Result<(), SpiError<'_>>> {
                Ready(Err(wrong_state()))
            }

            fn spi_capabilities(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<SpiCapabilities<'_>> {
                Ready(SpiCapabilities {
                    min_freq: ww_si::Hertz::from_parts(Prefix::Kilo, NumericValue::U32(100)),
                    max_freq: ww_si::Hertz::from_parts(Prefix::Mega, NumericValue::U32(10)),
                    min_word_bits: 8,
                    max_word_bits: 8,
                    master: false,
                    io_levels: MaxLen::new(RefVec::Slice { slice: &[] }).unwrap(),
                    io_level_min: None,
                    io_level_max: None,
                    max_len: ww_spi::SPI_MAX as u32,
                })
            }

            fn spi_configure(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                config: SpiConfig,
            ) -> RpcResult<Result<SpiConfig, SpiError<'_>>> {
                match config.role {
                    SpiRole::Disabled => Ready(Ok(SpiConfig {
                        role: SpiRole::Disabled,
                        io_level: None,
                        ..config
                    })),
                    _ => Ready(Err(CommonError::Unsupported.into())),
                }
            }

            fn spi_set_cs(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _active: bool,
            ) -> RpcResult<Result<(), SpiError<'_>>> {
                Ready(Err(wrong_state()))
            }

            fn spi_slave_capabilities(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<SpiSlaveCapabilities> {
                Ready(SpiSlaveCapabilities {
                    max_freq: ww_si::Hertz::from_parts(Prefix::Mega, NumericValue::U32(10)),
                    queue_len: Q as u32,
                    map_len: MAP as u32,
                    max_addr_bytes: 4,
                    map_max_freq: Some(ww_si::Hertz::from_parts(
                        Prefix::Mega,
                        NumericValue::U32(1),
                    )),
                })
            }

            fn spi_slave_frames(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                config: SpiSlaveConfig,
            ) -> RpcResult<Result<(), SpiError<'_>>> {
                Ready(self.engine.configure(&config).map_err(Into::into))
            }

            fn close_spi_slave_frames(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {
                self.engine.disable();
            }

            fn stream_spi_slave_frames(&mut self) -> &mut Counted {
                &mut self.frames
            }

            fn spi_slave_map_write(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                offset: u32,
                data: MaxLen<RefVec<'_, u8>, { ww_spi::SPI_MAX }>,
            ) -> RpcResult<Result<(), SpiError<'_>>> {
                Ready(
                    self.engine
                        .map_write(offset, data.as_slice())
                        .map_err(Into::into),
                )
            }

            fn spi_slave_map_read(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                offset: u32,
                len: u16,
            ) -> RpcResult<Result<MaxLen<RefVec<'_, u8>, { ww_spi::SPI_MAX }>, SpiError<'_>>>
            {
                let len = len as usize;
                if len > self.map_scratch.len() {
                    return Ready(Err(SpiError::BufferTooBig));
                }
                match self.engine.map_read(offset, &mut self.map_scratch[..len]) {
                    Ok(()) => Ready(Ok(
                        MaxLen::new(RefVec::new_bytes(&self.map_scratch[..len])).unwrap()
                    )),
                    Err(e) => Ready(Err(e.into())),
                }
            }

            fn spi_slave_clear_answers(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<u32> {
                Ready(self.engine.clear_answers())
            }

            fn credit_spi_slave_answers(&mut self) -> &mut SinkCredit {
                &mut self.slave_answers_credit
            }

            fn sideband_spi_slave_answers(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: StreamSideband,
            ) -> Option<StreamSideband> {
                None
            }

            fn write_spi_slave_answers(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                data: &shrink_wrap::tail_bytes::TailBytes<'_>,
            ) {
                self.engine
                    .push_answer(data.0)
                    .expect("test script respects slave_capabilities().queue_len and answer size");
                self.slave_answers_credit.free(data.0.len() as u32);
            }

            fn spi_slave_bus(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                op: SpiBusOp,
            ) -> RpcResult<SpiBusResult> {
                let now = self.now_us;
                Ready(match op {
                    SpiBusOp::Advance(us) => {
                        self.now_us = self.now_us.wrapping_add(us);
                        SpiBusResult::Advanced
                    }
                    SpiBusOp::CsAsserted => {
                        self.engine.cs_asserted(now);
                        SpiBusResult::Done
                    }
                    SpiBusOp::Miso => SpiBusResult::Byte(self.engine.miso()),
                    SpiBusOp::Mosi(byte) => {
                        self.engine.mosi(byte);
                        SpiBusResult::Done
                    }
                    SpiBusOp::CsReleased => {
                        self.engine.cs_released(now);
                        SpiBusResult::Done
                    }
                    SpiBusOp::Overrun => {
                        self.engine.overrun();
                        SpiBusResult::Done
                    }
                    SpiBusOp::Rewind => {
                        self.engine.rewind();
                        SpiBusResult::Done
                    }
                    SpiBusOp::Unknown(_) => unreachable!("test never sends an unknown op"),
                })
            }

            fn spi_slave_pump(
                &mut self,
                cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<()> {
                self.pump(cx);
                Ready(())
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                io_spi_api :: IoSpiSlaveTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                omit = ["spi.monitor"],
                method_model = "_=immediate",
                property_model = "_=get_set",
                // debug_to_file = "../../target/tests_io_spi_slave_server.rs"
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

    mod slave_std_client {
        use wire_weaver_client::{ClientConfig, Commander, WwClient};

        pub struct SlaveClient {
            pub cmd: Commander,
        }

        impl WwClient for SlaveClient {
            fn default_config() -> ClientConfig {
                ClientConfig::default()
            }

            fn from_cmd(cmd: Commander) -> Self {
                Self { cmd }
            }
        }

        mod api_client {
            wire_weaver::ww_codegen!(
                io_spi_api :: IoSpiSlaveTest for super::SlaveClient,
                client = "std_client",
            );
        }
    }

    use slave_std_client::SlaveClient;

    fn start_slave(path: &str) -> TestDevice {
        let _ = tracing_subscriber::fmt::try_init();
        let server = slave_mock::Server::new();
        tests_common::start_device(
            path,
            server,
            io_spi_api::IO_SPI_SLAVE_TEST_FULL_GID,
            slave_mock::api_impl::api_hash(),
        )
    }

    async fn connect_slave(device: &TestDevice) -> SlaveClient {
        SlaveClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    fn registers_config(
        addr_bytes: u8,
        rw: ww_spi::SpiRwBit,
        dummy_bytes: u8,
        len: u32,
        idle: u8,
    ) -> SpiSlaveConfig {
        SpiSlaveConfig {
            mode: SpiMode::Mode0,
            bit_order: BitOrder::MsbFirst,
            word_bits: 8,
            io_level: None,
            answers: SpiSlaveAnswers::Registers(ww_spi::SpiRegisters {
                addr_bytes,
                rw,
                dummy_bytes,
                idle,
                len,
            }),
        }
    }

    fn queue_slave_config(idle: u8, repeat_last: bool) -> SpiSlaveConfig {
        SpiSlaveConfig {
            mode: SpiMode::Mode0,
            bit_order: BitOrder::MsbFirst,
            word_bits: 8,
            io_level: None,
            answers: SpiSlaveAnswers::Queue { idle, repeat_last },
        }
    }

    use io_spi_api::{SpiBusOp, SpiBusResult};

    async fn bus(client: &mut SlaveClient, op: SpiBusOp) -> SpiBusResult {
        client.spi_slave_bus(op).call().await.unwrap()
    }

    /// One frame: CS asserted, `mosi.len()` bytes exchanged one at a time, CS released. Returns the MISO bytes.
    async fn frame(client: &mut SlaveClient, mosi: &[u8]) -> Vec<u8> {
        assert_eq!(bus(client, SpiBusOp::CsAsserted).await, SpiBusResult::Done);
        let mut miso = vec![];
        for &b in mosi {
            match bus(client, SpiBusOp::Miso).await {
                SpiBusResult::Byte(v) => miso.push(v),
                r => panic!("{r:?}"),
            }
            assert_eq!(bus(client, SpiBusOp::Mosi(b)).await, SpiBusResult::Done);
        }
        assert_eq!(bus(client, SpiBusOp::CsReleased).await, SpiBusResult::Done);
        miso
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn slave_registers_read_and_write_with_rw_bit() {
        let device = start_slave("io_spi_slave/registers");
        let mut client = connect_slave(&device).await;
        let (_, mut frames) = client
            .spi()
            .slave_frames(registers_config(
                1,
                ww_spi::SpiRwBit::SetIsRead { bit: 7 },
                0,
                64,
                0xEE,
            ))
            .call()
            .await
            .unwrap()
            .unwrap();

        client
            .spi()
            .slave_map_write(0x10, MaxLen::new(vec![1u8, 2, 3]).unwrap())
            .call()
            .await
            .unwrap();

        // bit 7 set: a read at 0x10
        assert_eq!(
            frame(&mut client, &[0x90, 0, 0, 0]).await,
            vec![0xEE, 1, 2, 3]
        );
        // bit 7 clear: a write at 0x20
        assert_eq!(frame(&mut client, &[0x20, 9, 8]).await, vec![0xEE; 3]);
        let map = client.spi().slave_map_read(0x20, 2).call().await.unwrap();
        assert_eq!(map.as_slice(), &[9, 8]);

        client.spi_slave_pump().call().await.unwrap();
        let f = frames.recv().await.unwrap();
        assert_eq!(f.addr, Some(0x10));
        assert_eq!(f.mosi.as_slice(), &[0x90, 0, 0, 0]);
        let f = frames.recv().await.unwrap();
        assert_eq!(f.addr, Some(0x20));
        assert_eq!(f.mosi.as_slice(), &[0x20, 9, 8]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn slave_host_map_write_applies_between_frames_not_inside() {
        let device = start_slave("io_spi_slave/host_update");
        let mut client = connect_slave(&device).await;
        let (_, _frames) = client
            .spi()
            .slave_frames(registers_config(1, ww_spi::SpiRwBit::ReadOnly, 0, 64, 0xEE))
            .call()
            .await
            .unwrap()
            .unwrap();
        client
            .spi()
            .slave_map_write(0, MaxLen::new(vec![1u8, 1]).unwrap())
            .call()
            .await
            .unwrap();

        assert_eq!(
            bus(&mut client, SpiBusOp::CsAsserted).await,
            SpiBusResult::Done
        );
        assert_eq!(
            bus(&mut client, SpiBusOp::Miso).await,
            SpiBusResult::Byte(0xEE)
        ); // address byte
        assert_eq!(
            bus(&mut client, SpiBusOp::Mosi(0)).await,
            SpiBusResult::Done
        );
        client
            .spi()
            .slave_map_write(0, MaxLen::new(vec![2u8, 2]).unwrap())
            .call()
            .await
            .unwrap();
        assert_eq!(
            bus(&mut client, SpiBusOp::Miso).await,
            SpiBusResult::Byte(1)
        );
        assert_eq!(
            bus(&mut client, SpiBusOp::Mosi(0)).await,
            SpiBusResult::Done
        );
        assert_eq!(
            bus(&mut client, SpiBusOp::Miso).await,
            SpiBusResult::Byte(1)
        );
        assert_eq!(
            bus(&mut client, SpiBusOp::Mosi(0)).await,
            SpiBusResult::Done
        );
        assert_eq!(
            bus(&mut client, SpiBusOp::CsReleased).await,
            SpiBusResult::Done
        );

        let map = client.spi().slave_map_read(0, 2).call().await.unwrap();
        assert_eq!(map.as_slice(), &[2, 2]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn slave_answer_queue_one_per_frame_idle_for_rest() {
        let device = start_slave("io_spi_slave/answer_queue");
        let mut client = connect_slave(&device).await;
        let (_, mut frames) = client
            .spi()
            .slave_frames(queue_slave_config(0x5A, false))
            .call()
            .await
            .unwrap()
            .unwrap();

        let mut answers = client.spi().slave_answers().await.unwrap();
        answers.send_bytes(&[1, 2]).await.unwrap();
        answers.send_bytes(&[3, 4, 5]).await.unwrap();
        answers.flush().await.unwrap();

        assert_eq!(
            frame(&mut client, &[0xA0, 0xA1, 0xA2]).await,
            vec![1, 2, 0x5A]
        );
        // shorter frame: the rest of the second answer is dropped
        assert_eq!(frame(&mut client, &[0, 0]).await, vec![3, 4]);
        // nothing queued: idle, underrun
        assert_eq!(frame(&mut client, &[0, 0]).await, vec![0x5A, 0x5A]);

        client.spi_slave_pump().call().await.unwrap();
        let f0 = frames.recv().await.unwrap();
        assert_eq!(f0.mosi.as_slice(), &[0xA0, 0xA1, 0xA2]);
        assert_eq!(f0.miso.as_slice(), &[1, 2, 0x5A]);
        assert!(!f0.underrun);
        let f1 = frames.recv().await.unwrap();
        assert!(!f1.underrun);
        let f2 = frames.recv().await.unwrap();
        assert!(f2.underrun);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn slave_answer_queue_repeat_last() {
        let device = start_slave("io_spi_slave/repeat_last");
        let mut client = connect_slave(&device).await;
        let (_, mut frames) = client
            .spi()
            .slave_frames(queue_slave_config(0, true))
            .call()
            .await
            .unwrap()
            .unwrap();

        // nothing ever queued: idle
        assert_eq!(frame(&mut client, &[0, 0]).await, vec![0, 0]);
        let mut answers = client.spi().slave_answers().await.unwrap();
        answers.send_bytes(&[7, 8]).await.unwrap();
        answers.flush().await.unwrap();
        assert_eq!(frame(&mut client, &[0, 0]).await, vec![7, 8]);
        // kept to repeat
        assert_eq!(frame(&mut client, &[0, 0, 0]).await, vec![7, 8, 0]);
        answers.send_bytes(&[9]).await.unwrap();
        answers.flush().await.unwrap();
        assert_eq!(frame(&mut client, &[0]).await, vec![9]);

        client.spi_slave_pump().call().await.unwrap();
        let mut underrun = vec![];
        for _ in 0..4 {
            underrun.push(frames.recv().await.unwrap().underrun);
        }
        assert_eq!(underrun, vec![true, false, true, false]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn slave_clear_answers_drops_only_waiting() {
        let device = start_slave("io_spi_slave/clear_answers");
        let mut client = connect_slave(&device).await;
        let (_, _frames) = client
            .spi()
            .slave_frames(queue_slave_config(0xEE, false))
            .call()
            .await
            .unwrap()
            .unwrap();

        let mut answers = client.spi().slave_answers().await.unwrap();
        for b in [0u8, 1, 2] {
            answers.send_bytes(&[b]).await.unwrap();
        }
        answers.flush().await.unwrap();

        // the answer picked for this frame (preloaded at the first `miso`) stays, the rest are dropped
        assert_eq!(
            bus(&mut client, SpiBusOp::Miso).await,
            SpiBusResult::Byte(0)
        );
        let dropped = client.spi().slave_clear_answers().call().await.unwrap();
        assert_eq!(dropped, 2);
        assert_eq!(
            bus(&mut client, SpiBusOp::CsAsserted).await,
            SpiBusResult::Done
        );
        assert_eq!(
            bus(&mut client, SpiBusOp::Mosi(0)).await,
            SpiBusResult::Done
        );
        assert_eq!(
            bus(&mut client, SpiBusOp::CsReleased).await,
            SpiBusResult::Done
        );

        // the cleared answers never come back: the next frame is idle only
        assert_eq!(frame(&mut client, &[0]).await, vec![0xEE]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn slave_dropped_frames_are_counted() {
        let device = start_slave("io_spi_slave/dropped");
        let mut client = connect_slave(&device).await;
        let (_, mut frames) = client
            .spi()
            .slave_frames(registers_config(0, ww_spi::SpiRwBit::ReadOnly, 0, 4, 0))
            .call()
            .await
            .unwrap()
            .unwrap();

        // the frame ring holds 8 (EV): 10 one-byte frames without a pump overflow it by 2
        for _ in 0..10 {
            assert_eq!(frame(&mut client, &[0]).await, vec![0]);
        }
        client.spi_slave_pump().call().await.unwrap();

        let mut seen = 0;
        for _ in 0..8 {
            match frames.recv_event().await.unwrap() {
                wire_weaver_client::StreamEvent::Item { .. } => seen += 1,
                ev => panic!("{ev:?}"),
            }
        }
        assert_eq!(seen, 8);

        // the drop is reported after the 8 it kept, not before: it only shows up once there is a next frame to
        // carry it
        assert_eq!(frame(&mut client, &[0]).await, vec![0]);
        client.spi_slave_pump().call().await.unwrap();
        match frames.recv_event().await.unwrap() {
            wire_weaver_client::StreamEvent::Gap { units, .. } => assert_eq!(units, 2),
            ev => panic!("{ev:?}"),
        }
    }
}

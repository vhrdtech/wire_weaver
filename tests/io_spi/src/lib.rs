//! `ww_spi` end to end: a mock server on `ww_device::Server` through `tests_common::start_device`, the generated
//! client through the real host event loop.

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use ww_client_server::StreamSideband;
    use ww_numeric::NumericValue;
    use ww_si::{Prefix, Second, Volt};
    use ww_spi::{
        BitOrder, CsMode, SpiActual, SpiCapabilities, SpiConfig, SpiError, SpiFrame, SpiMode,
        SpiMonitorConfig, SpiOp, SpiRole,
    };

    /// Smaller than [ww_spi::SPI_MAX], so a too-long transfer can be tested without building a 512-byte buffer.
    const MAX_LEN: usize = 64;

    fn volt(value: f32) -> Volt {
        Volt {
            prefix: Prefix::Unit,
            value: NumericValue::F32(value),
        }
    }

    fn second(prefix: Prefix, value: u32) -> Second {
        Second {
            prefix,
            value: NumericValue::U32(value),
        }
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
        reference_voltage: Option<Volt>,
        /// Bytes of every `Spi::write`, in order.
        writes: Vec<Vec<u8>>,
        monitor_config: Option<SpiMonitorConfig>,
        monitor_sideband: Vec<StreamSideband>,
    }

    mod no_std_sync_server {
        use super::*;
        use tests_common::TestProcessEvents;

        pub struct NoStdSyncServer {
            pub data: Arc<Mutex<Model>>,
        }

        impl NoStdSyncServer {
            fn spi_transfer(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                data: MaxLen<RefVec<'_, u8>, { ww_spi::SPI_MAX }>,
            ) -> RpcResult<Result<MaxLen<RefVec<'_, u8>, { ww_spi::SPI_MAX }>, SpiError>>
            {
                let master = matches!(
                    self.data.lock().unwrap().configured.map(|c| c.role),
                    Some(SpiRole::Master)
                );
                if !master {
                    return Ready(Err(SpiError::WrongMode));
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
            ) -> RpcResult<Result<MaxLen<RefVec<'_, u8>, { ww_spi::SPI_MAX }>, SpiError>>
            {
                let master = matches!(
                    self.data.lock().unwrap().configured.map(|c| c.role),
                    Some(SpiRole::Master)
                );
                if !master {
                    return Ready(Err(SpiError::WrongMode));
                }
                let mut read_bytes = Vec::new();
                for op in ops.into_inner().iter() {
                    match op.unwrap() {
                        SpiOp::Write(_) => {}
                        // the mock peripheral has nothing to say: reads come back as 0xA5
                        SpiOp::Read(len) => {
                            read_bytes.extend(std::iter::repeat(0xA5u8).take(len as usize))
                        }
                        SpiOp::Transfer(data) => {
                            read_bytes.extend_from_slice(data.into_inner().as_slice())
                        }
                        SpiOp::Delay(_) => {}
                    }
                }
                Ready(Ok(MaxLen::new(leak_bytes(read_bytes)).unwrap()))
            }

            fn spi_write(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                data: MaxLen<RefVec<'_, u8>, { ww_spi::SPI_MAX }>,
            ) -> RpcResult<Result<(), SpiError>> {
                let master = matches!(
                    self.data.lock().unwrap().configured.map(|c| c.role),
                    Some(SpiRole::Master)
                );
                if !master {
                    return Ready(Err(SpiError::WrongMode));
                }
                let bytes = data.into_inner().as_slice().to_vec();
                self.data.lock().unwrap().writes.push(bytes);
                Ready(Ok(()))
            }

            fn spi_capabilities(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<SpiCapabilities<'_>> {
                const IO_LEVELS: [Volt; 1] = [Volt {
                    prefix: Prefix::Unit,
                    value: NumericValue::F32(3.3),
                }];
                Ready(SpiCapabilities {
                    min_freq: ww_si::Hertz {
                        prefix: Prefix::Kilo,
                        value: NumericValue::U32(100),
                    },
                    max_freq: ww_si::Hertz {
                        prefix: Prefix::Mega,
                        value: NumericValue::U32(10),
                    },
                    min_word_bits: 8,
                    max_word_bits: 8,
                    master: true,
                    slave: false,
                    monitor: true,
                    io_levels: MaxLen::new(RefVec::Slice { slice: &IO_LEVELS }).unwrap(),
                    max_len: MAX_LEN as u32,
                })
            }

            fn spi_configure(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                config: SpiConfig,
            ) -> RpcResult<Result<SpiActual, SpiError>> {
                if matches!(config.role, SpiRole::Slave) {
                    return Ready(Err(SpiError::Unsupported));
                }
                if config.word_bits != 8 {
                    return Ready(Err(SpiError::InvalidArgument));
                }
                let freq = config.freq;
                self.data.lock().unwrap().configured = Some(config);
                Ready(Ok(SpiActual { freq }))
            }

            fn spi_set_cs(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                active: bool,
            ) -> RpcResult<Result<(), SpiError>> {
                let mut model = self.data.lock().unwrap();
                let manual = matches!(model.configured.map(|c| c.cs), Some(CsMode::Manual));
                if !manual {
                    return Ready(Err(SpiError::WrongMode));
                }
                model.cs_active = active;
                Ready(Ok(()))
            }

            fn set_spi_reference_voltage(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Volt,
            ) -> SetResult<SpiError> {
                let supported =
                    matches!(value.value, NumericValue::F32(v) if (v - 3.3).abs() < 0.01);
                if !supported {
                    return SetError(SpiError::Unsupported);
                }
                self.data.lock().unwrap().reference_voltage = Some(value);
                Set
            }

            fn get_spi_reference_voltage(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Volt, SpiError> {
                Value(
                    self.data
                        .lock()
                        .unwrap()
                        .reference_voltage
                        .unwrap_or(volt(3.3)),
                )
            }

            fn sideband_spi_monitor_frames(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                sideband: StreamSideband,
            ) -> Option<StreamSideband> {
                self.data.lock().unwrap().monitor_sideband.push(sideband);
                None
            }

            fn spi_monitor_configure_monitor(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                config: SpiMonitorConfig,
            ) -> RpcResult<Result<(), SpiError>> {
                if config.word_bits != 8 {
                    return Ready(Err(SpiError::Unsupported));
                }
                self.data.lock().unwrap().monitor_config = Some(config);
                Ready(Ok(()))
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                io_spi_api :: IoSpiTest for super::NoStdSyncServer,
                server = true, no_alloc = true, use_async = false,
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

    use no_std_sync_server::api_impl::stream_data_ser;
    use std_client::StdClient;

    fn start(path: &str) -> (TestDevice, Arc<Mutex<Model>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let data = Arc::new(Mutex::new(Model::default()));
        let server = no_std_sync_server::NoStdSyncServer { data: data.clone() };
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
            freq: ww_si::Hertz {
                prefix: Prefix::Mega,
                value: NumericValue::U32(1),
            },
            mode: SpiMode::Mode0,
            bit_order: BitOrder::MsbFirst,
            word_bits: 8,
            cs,
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn capabilities_and_configure() {
        let (device, _data) = start("io_spi/capabilities_and_configure");
        let client = connect(&device).await;

        let caps = client.spi().capabilities().call().await.unwrap();
        assert!(caps.master);
        assert!(!caps.slave);
        assert_eq!(caps.max_len, MAX_LEN as u32);

        let actual = client
            .spi()
            .configure(configured_master(CsMode::Auto))
            .call()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(actual.freq.value, NumericValue::U32(1));
        assert_eq!(actual.freq.prefix, Prefix::Mega);

        // this mock has no slave mode
        let mut slave = configured_master(CsMode::Auto);
        slave.role = SpiRole::Slave;
        let err = client.spi().configure(slave).call().await.unwrap();
        assert!(matches!(err, Err(SpiError::Unsupported)));
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
            .unwrap()
            .unwrap();

        let echoed = client
            .spi()
            .transfer(MaxLen::new(vec![1u8, 2, 3]).unwrap())
            .call()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(echoed.into_inner(), vec![1, 2, 3]);

        client
            .spi()
            .write(MaxLen::new(vec![9u8, 8]).unwrap())
            .call()
            .await
            .unwrap()
            .unwrap();
        eventually(&data, |d| !d.writes.is_empty()).await;
        assert_eq!(data.lock().unwrap().writes, vec![vec![9u8, 8]]);

        let too_long = vec![0u8; MAX_LEN + 1];
        let err = client
            .spi()
            .transfer(MaxLen::new(too_long).unwrap())
            .call()
            .await
            .unwrap();
        assert_eq!(err, Err(SpiError::BufferTooBig));
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
            .unwrap()
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
            .unwrap()
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
            .unwrap()
            .unwrap();

        let err = client.spi().set_cs(true).call().await.unwrap();
        assert_eq!(err, Err(SpiError::WrongMode));

        client
            .spi()
            .configure(configured_master(CsMode::Manual))
            .call()
            .await
            .unwrap()
            .unwrap();
        client.spi().set_cs(true).call().await.unwrap().unwrap();
        assert!(data.lock().unwrap().cs_active);
        client.spi().set_cs(false).call().await.unwrap().unwrap();
        assert!(!data.lock().unwrap().cs_active);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reference_voltage_property() {
        let (device, _data) = start("io_spi/reference_voltage");
        let client = connect(&device).await;

        // default before it is ever set
        let v = client.spi().read_reference_voltage().read().await.unwrap();
        assert_eq!(v.value, NumericValue::F32(3.3));

        let err = client
            .spi()
            .write_reference_voltage(volt(5.0))
            .write()
            .await;
        assert!(err.is_err());

        client
            .spi()
            .write_reference_voltage(volt(3.3))
            .write()
            .await
            .unwrap();
        let v = client.spi().read_reference_voltage().read().await.unwrap();
        assert_eq!(v.value, NumericValue::F32(3.3));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn monitor_configure_and_frames_stream() {
        let (device, data) = start("io_spi/monitor");
        let client = connect(&device).await;
        let mut stream = client.spi_monitor().frames().await.unwrap();

        client
            .spi_monitor()
            .configure_monitor(SpiMonitorConfig {
                mode: SpiMode::Mode0,
                bit_order: BitOrder::MsbFirst,
                word_bits: 8,
            })
            .call()
            .await
            .unwrap()
            .unwrap();

        stream.open().await.unwrap();
        eventually(&data, |d| !d.monitor_sideband.is_empty()).await;
        assert_eq!(
            data.lock().unwrap().monitor_sideband,
            vec![StreamSideband::Open]
        );

        let mosi = [0x9Fu8];
        let miso = [0x00u8, 0x00, 0x40];
        let frame = SpiFrame {
            start: second(Prefix::Micro, 100),
            end: second(Prefix::Micro, 140),
            truncated: false,
            mosi: MaxLen::new(RefVec::Slice { slice: &mosi }).unwrap(),
            miso: MaxLen::new(RefVec::Slice { slice: &miso }).unwrap(),
        };
        let mut scratch = [0u8; 64];
        device.send(
            stream_data_ser()
                .spi_monitor()
                .frames(&frame, &mut scratch)
                .unwrap(),
        );
        let received = stream.recv().await.unwrap();
        assert_eq!(received.mosi.into_inner(), vec![0x9F]);
        assert_eq!(received.miso.into_inner(), vec![0x00, 0x00, 0x40]);
        assert!(!received.truncated);

        stream.close().await.unwrap();
        eventually(&data, |d| d.monitor_sideband.len() == 2).await;
        assert_eq!(
            data.lock().unwrap().monitor_sideband[1],
            StreamSideband::Close
        );
    }
}

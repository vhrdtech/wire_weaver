//! `ww_uart` end to end: a mock server on `ww_device::Server` through `tests_common::start_device`, the generated
//! client through the real host event loop.

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver_client::StreamEvent;
    use ww_client_server::StreamSideband;
    use ww_stream::tx::Counted;
    use ww_uart::{
        BaudRate, Capabilities, DataBits, Error, Mode, MonitorChunk, MonitorConfig, MonitorLine,
        Parity, Pin, RxChunk, StopBits,
    };

    /// Device behaviour and what it received, shared with the test.
    struct Model {
        baud_rate: BaudRate,
        mode: Mode,
        stop_bits: StopBits,
        parity: Parity,
        data_bits: DataBits,
        prevent_back_feed: bool,
        reference_voltage: ww_si::Volt,
        /// `(pin, is_high)` of every `set_pin_level` call accepted in [Mode::Test].
        pin_levels: Vec<(Pin, bool)>,
        send_break_calls: Vec<ww_si::Second>,
        tx_bytes: Vec<u8>,
        tx_sideband: Vec<StreamSideband>,
        /// The `rx` stream is open.
        rx_open: bool,
        /// Set while `UartMonitor::lines` is open.
        monitor_config: Option<MonitorConfig>,
        /// Bytes the test makes arrive, sent on `rx` or `lines` (with `Some(line)`) before the next request.
        received: Vec<(Option<MonitorLine>, Vec<u8>)>,
    }

    impl Default for Model {
        fn default() -> Self {
            Model {
                baud_rate: BaudRate::Baud115200,
                mode: Mode::Asynchronous,
                stop_bits: StopBits::Stop1,
                parity: Parity::None,
                data_bits: DataBits::Eight,
                prevent_back_feed: false,
                reference_voltage: ww_si::quantity!(3300 mV u16),
                pin_levels: vec![],
                send_break_calls: vec![],
                tx_bytes: vec![],
                tx_sideband: vec![],
                rx_open: false,
                monitor_config: None,
                received: vec![],
            }
        }
    }

    /// Fixed, device-wide limits reported by `Uart::capabilities`.
    fn capabilities() -> Capabilities<'static> {
        Capabilities {
            min_baud_rate: 110,
            max_baud_rate: 921_600,
            voltages: MaxLen::new(RefVec::Slice {
                slice: &[ww_si::quantity!(3300 mV u16)],
            })
            .unwrap(),
            rx_timestamps: true,
            hw_flow_control: true,
            sw_flow_control: false,
            high_z_mode: true,
            test_mode: true,
            back_feed_detector: true,
            rx_buffer: 1024,
            tx_buffer: ww_uart::UART_TX_MAX as u32,
        }
    }

    mod sync_server {
        use super::*;
        use tests_common::TestProcessEvents;
        use ww_client_server::credit::SinkCredit;

        pub struct Server {
            pub m: Arc<Mutex<Model>>,
            pub uart_tx_credit: SinkCredit,
            pub rx: Counted,
            pub lines: Counted,
            pub stream_sessions: u32,
            pub start: std::time::Instant,
        }

        impl Server {
            /// The event loop's producer: bytes the lines received, as chunks of the open streams.
            fn produce(&mut self, out: &mut impl BlockingEventOut) {
                let received = std::mem::take(&mut self.m.lock().unwrap().received);
                let t = ww_si::Second::microseconds(self.start.elapsed().as_micros() as u32);
                let ser = api_impl::stream_data_ser();
                for (line, bytes) in received {
                    let chunk = RxChunk {
                        flags: None,
                        bytes: MaxLen::new(RefVec::new_bytes(&bytes)).unwrap(),
                    };
                    let units = bytes.len() as u64;
                    let _ = match line {
                        None => {
                            ser.uart()
                                .rx_send_blocking(&mut self.rx, &chunk, units, Some(t), out)
                        }
                        Some(line) => ser.uart_monitor().lines_send_blocking(
                            &mut self.lines,
                            &MonitorChunk { line, chunk },
                            units,
                            Some(t),
                            out,
                        ),
                    };
                }
            }

            fn new_stream_session(&mut self) -> u32 {
                self.stream_sessions = self.stream_sessions.wrapping_add(1);
                self.stream_sessions
            }

            fn uart_rx(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
                self.m.lock().unwrap().rx_open = true;
                Ready(())
            }

            fn close_uart_rx(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {
                self.m.lock().unwrap().rx_open = false;
            }

            fn stream_uart_rx(&mut self) -> &mut Counted {
                &mut self.rx
            }

            fn credit_uart_tx(&mut self) -> &mut SinkCredit {
                &mut self.uart_tx_credit
            }

            fn sideband_uart_tx(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                sideband: StreamSideband,
            ) -> Option<StreamSideband> {
                self.m.lock().unwrap().tx_sideband.push(sideband);
                None
            }

            fn write_uart_tx(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                data: &shrink_wrap::tail_bytes::TailBytes<'_>,
            ) {
                let mut m = self.m.lock().unwrap();
                m.tx_bytes.extend_from_slice(data.0);
                self.uart_tx_credit.free(data.0.len() as u32);
            }

            fn uart_capabilities(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Capabilities<'_>> {
                Ready(capabilities())
            }

            fn set_uart_baud_rate(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: BaudRate,
            ) -> SetResult<Error> {
                if matches!(value, BaudRate::BaudOther(hz) if hz > capabilities().max_baud_rate) {
                    return SetResult::SetError(Error::Unsupported);
                }
                self.m.lock().unwrap().baud_rate = value;
                Set
            }

            fn get_uart_baud_rate(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<BaudRate, Error> {
                Value(self.m.lock().unwrap().baud_rate)
            }

            fn set_uart_mode(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Mode,
            ) -> SetResult<Error> {
                self.m.lock().unwrap().mode = value;
                Set
            }

            fn get_uart_mode(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Mode, Error> {
                Value(self.m.lock().unwrap().mode)
            }

            fn set_uart_stop_bits(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: StopBits,
            ) -> SetResult<Error> {
                self.m.lock().unwrap().stop_bits = value;
                Set
            }

            fn get_uart_stop_bits(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<StopBits, Error> {
                Value(self.m.lock().unwrap().stop_bits)
            }

            fn set_uart_parity(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Parity,
            ) -> SetResult<Error> {
                self.m.lock().unwrap().parity = value;
                Set
            }

            fn get_uart_parity(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Parity, Error> {
                Value(self.m.lock().unwrap().parity)
            }

            fn set_uart_data_bits(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: DataBits,
            ) -> SetResult<Error> {
                self.m.lock().unwrap().data_bits = value;
                Set
            }

            fn get_uart_data_bits(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<DataBits, Error> {
                Value(self.m.lock().unwrap().data_bits)
            }

            fn set_uart_prevent_back_feed(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: bool,
            ) -> SetResult<Error> {
                self.m.lock().unwrap().prevent_back_feed = value;
                Set
            }

            fn get_uart_prevent_back_feed(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<bool, Error> {
                Value(self.m.lock().unwrap().prevent_back_feed)
            }

            fn set_uart_reference_voltage(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: ww_si::Volt,
            ) -> SetResult<Error> {
                self.m.lock().unwrap().reference_voltage = value;
                Set
            }

            fn get_uart_reference_voltage(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<ww_si::Volt, Error> {
                Value(self.m.lock().unwrap().reference_voltage)
            }

            fn uart_set_pin_level(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                pin: Pin,
                is_high: bool,
            ) -> RpcResult<Result<(), Error>> {
                let mut m = self.m.lock().unwrap();
                if !matches!(m.mode, Mode::Test) {
                    return Ready(Err(Error::WrongMode));
                }
                m.pin_levels.push((pin, is_high));
                Ready(Ok(()))
            }

            fn uart_baud_rate_actual(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<u32> {
                let actual = match self.m.lock().unwrap().baud_rate {
                    BaudRate::Baud110 => 110,
                    BaudRate::Baud300 => 300,
                    BaudRate::Baud600 => 600,
                    BaudRate::Baud1200 => 1200,
                    BaudRate::Baud2400 => 2400,
                    BaudRate::Baud4800 => 4800,
                    BaudRate::Baud9600 => 9600,
                    BaudRate::Baud19200 => 19200,
                    BaudRate::Baud38400 => 38400,
                    BaudRate::Baud57600 => 57600,
                    BaudRate::Baud115200 => 115200,
                    BaudRate::Baud921600 => 921_600,
                    BaudRate::BaudOther(hz) => hz,
                };
                Ready(actual)
            }

            fn uart_send_break(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                duration: ww_si::Second,
            ) -> RpcResult<Result<(), Error>> {
                self.m.lock().unwrap().send_break_calls.push(duration);
                Ready(Ok(()))
            }

            fn uart_tx_pending(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<u32> {
                Ready(self.uart_tx_credit.buffered())
            }

            fn uart_monitor_lines(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                config: MonitorConfig,
            ) -> RpcResult<Result<u32, Error>> {
                let actual = match config.baud_rate {
                    BaudRate::Baud115200 => 115200,
                    BaudRate::BaudOther(hz) => hz,
                    BaudRate::Baud110 => return Ready(Err(Error::Unsupported)),
                    _ => 9600,
                };
                self.m.lock().unwrap().monitor_config = Some(config);
                Ready(Ok(actual))
            }

            fn close_uart_monitor_lines(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {
                self.m.lock().unwrap().monitor_config = None;
            }

            fn stream_uart_monitor_lines(&mut self) -> &mut Counted {
                &mut self.lines
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                io_uart_api :: IoUartTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                // debug_to_file = "../../target/tests_io_uart_server.rs"
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
                io_uart_api :: IoUartTest for super::StdClient,
                client = "std_client",
                // debug_to_file = "../../target/tests_io_uart_client.rs"
            );
        }
    }

    use client::StdClient;

    fn start(path: &str, model: Model) -> (TestDevice, Arc<Mutex<Model>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let m = Arc::new(Mutex::new(model));
        let server = sync_server::Server {
            m: m.clone(),
            uart_tx_credit: ww_client_server::credit::SinkCredit::new(ww_uart::UART_TX_MAX as u32),
            rx: Counted::new(),
            lines: Counted::new(),
            stream_sessions: 0,
            start: std::time::Instant::now(),
        };
        let device = tests_common::start_device(
            path,
            server,
            io_uart_api::IO_UART_TEST_FULL_GID,
            sync_server::api_impl::api_hash(),
        );
        (device, m)
    }

    async fn connect(device: &TestDevice) -> StdClient {
        StdClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    /// Wait for the device thread to process what was sent to it
    async fn eventually(m: &Arc<Mutex<Model>>, f: impl Fn(&Model) -> bool) {
        for _ in 0..500 {
            if f(&m.lock().unwrap()) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        panic!("condition not met within 500ms");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn capabilities_and_properties() {
        let (device, m) = start("io_uart/properties", Model::default());
        let client = connect(&device).await;

        let caps = client.uart().capabilities().call().await.unwrap();
        assert_eq!(caps.min_baud_rate, 110);
        assert_eq!(caps.max_baud_rate, 921_600);

        client
            .uart()
            .write_baud_rate(BaudRate::Baud9600)
            .write()
            .await
            .unwrap();
        assert!(matches!(m.lock().unwrap().baud_rate, BaudRate::Baud9600));
        let baud = client.uart().read_baud_rate().read().await.unwrap();
        assert!(matches!(baud, BaudRate::Baud9600));

        client
            .uart()
            .write_mode(Mode::HardwareFlowControl)
            .write()
            .await
            .unwrap();
        assert!(matches!(m.lock().unwrap().mode, Mode::HardwareFlowControl));

        let actual = client.uart().baud_rate_actual().call().await.unwrap();
        assert_eq!(actual, 9600);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn set_pin_level_requires_test_mode() {
        let (device, m) = start("io_uart/pin_level", Model::default());
        let client = connect(&device).await;

        let r = client
            .uart()
            .set_pin_level(Pin::Tx, true)
            .call()
            .await
            .unwrap();
        assert!(matches!(r, Err(Error::WrongMode)));
        assert!(m.lock().unwrap().pin_levels.is_empty());

        client.uart().write_mode(Mode::Test).write().await.unwrap();
        let r = client
            .uart()
            .set_pin_level(Pin::Tx, true)
            .call()
            .await
            .unwrap();
        assert!(matches!(r, Ok(())));
        assert_eq!(m.lock().unwrap().pin_levels, vec![(Pin::Tx, true)]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn send_break_and_tx_pending() {
        let (device, m) = start("io_uart/break", Model::default());
        let client = connect(&device).await;

        let duration = ww_si::quantity!(10 s u32);
        let r = client.uart().send_break(duration).call().await.unwrap();
        assert!(matches!(r, Ok(())));
        assert_eq!(m.lock().unwrap().send_break_calls.len(), 1);

        let pending = client.uart().tx_pending().call().await.unwrap();
        assert_eq!(pending, 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tx_credit_sink_and_rx_stream() {
        let (device, m) = start("io_uart/tx_rx", Model::default());
        let client = connect(&device).await;

        let mut tx = client.uart().tx().await.unwrap();
        tx.send_bytes(b"hello").await.unwrap();
        tx.flush().await.unwrap();
        assert_eq!(m.lock().unwrap().tx_bytes, b"hello");

        let mut rx = client.uart().rx().call().await.unwrap();
        assert!(m.lock().unwrap().rx_open);

        m.lock().unwrap().received.push((None, b"abc".to_vec()));
        m.lock().unwrap().received.push((None, b"de".to_vec()));
        // any request lets the mock's event loop send them
        client.uart().tx_pending().call().await.unwrap();
        for (pos, bytes) in [(0, &b"abc"[..]), (3, b"de")] {
            match rx.recv_event().await.unwrap() {
                StreamEvent::Item {
                    pos: p,
                    units,
                    t,
                    value,
                    ..
                } => {
                    assert_eq!((p, units), (pos, bytes.len() as u64));
                    assert!(t.is_some());
                    assert_eq!(value.bytes.into_inner().as_slice(), bytes);
                }
                ev => panic!("{ev:?}"),
            }
        }
        rx.close().await.unwrap();
        eventually(&m, |m| !m.rx_open).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn monitor_lines() {
        let (device, m) = start("io_uart/monitor", Model::default());
        let client = connect(&device).await;

        let config = |baud_rate| MonitorConfig {
            baud_rate,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::Stop1,
        };
        let r = client
            .uart_monitor()
            .lines(config(BaudRate::Baud110))
            .call()
            .await
            .unwrap();
        assert!(matches!(r, Err(Error::Unsupported)), "{:?}", r.err());

        let (actual, mut lines) = client
            .uart_monitor()
            .lines(config(BaudRate::Baud115200))
            .call()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(actual, 115200);
        assert!(matches!(
            m.lock()
                .unwrap()
                .monitor_config
                .as_ref()
                .map(|c| c.baud_rate),
            Some(BaudRate::Baud115200)
        ));

        // a request on line A, the answer on line B, in the order they arrived
        m.lock()
            .unwrap()
            .received
            .push((Some(MonitorLine::A), b"xyz".to_vec()));
        m.lock()
            .unwrap()
            .received
            .push((Some(MonitorLine::B), b"ok".to_vec()));
        client.uart().tx_pending().call().await.unwrap();
        let a = lines.recv().await.unwrap();
        assert_eq!(a.line, MonitorLine::A);
        assert_eq!(a.chunk.bytes.into_inner().as_slice(), b"xyz");
        let b = lines.recv().await.unwrap();
        assert_eq!(b.line, MonitorLine::B);
        assert_eq!(b.chunk.bytes.into_inner().as_slice(), b"ok");

        lines.close().await.unwrap();
        eventually(&m, |m| m.monitor_config.is_none()).await;
    }
}

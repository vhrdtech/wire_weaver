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
        Baud, Capabilities, DataBits, Mode, MonitorChunk, MonitorConfig, MonitorLine, Parity, Pin,
        RxChunk, StopBits, UartConfig, UartError, UartErrorOwned,
    };

    /// One character at 115 200 Bd, in µs.
    const CHAR_TIME_US: u32 = 87;

    /// Bytes that arrive back to back on a line.
    struct Burst {
        line: Option<MonitorLine>,
        bytes: Vec<u8>,
        /// The character time the sender actually ran at, in µs, which the mock measures and reports as
        /// `RxChunk::end`; `None`: the mock doesn't measure it.
        measured_char_us: Option<u32>,
    }

    impl Burst {
        fn rx(bytes: &[u8], measured_char_us: Option<u32>) -> Self {
            Burst {
                line: None,
                bytes: bytes.to_vec(),
                measured_char_us,
            }
        }
    }

    /// Device behaviour and what it received, shared with the test.
    struct Model {
        /// What `configure` set last.
        config: UartConfig,
        /// `(pin, is_high)` of every `set_pin_level` call accepted in [Mode::Test].
        pin_levels: Vec<(Pin, bool)>,
        send_break_calls: Vec<ww_si::Second>,
        tx_bytes: Vec<u8>,
        tx_sideband: Vec<StreamSideband>,
        /// The `rx` stream is open.
        rx_open: bool,
        /// Set while `Uart::monitor` is open.
        monitor_config: Option<MonitorConfig>,
        /// Bursts the test makes arrive, sent on `rx` or `lines` (with `Some(line)`) before the next request.
        received: Vec<Burst>,
    }

    impl Default for Model {
        fn default() -> Self {
            Model {
                config: UartConfig {
                    baud_rate: Baud::baud(115_200u32),
                    data_bits: DataBits::Eight,
                    parity: Parity::None,
                    stop_bits: StopBits::Stop1,
                    mode: Mode::Asynchronous,
                    prevent_back_feed: false,
                    io_level: Some(ww_si::quantity!(3300 mV u16)),
                    idle_chars: ww_uart::IDLE_CHARS_DEFAULT,
                },
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
            min_baud_rate: Baud::baud(110u32),
            max_baud_rate: Baud::baud(921_600u32),
            io_levels: MaxLen::new(RefVec::Slice {
                slice: &[ww_si::quantity!(1800 mV u16), ww_si::quantity!(3300 mV u16)],
            })
            .unwrap(),
            io_level_min: None,
            io_level_max: None,
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
                // the mock's line runs at about 115 200 Bd: 87 µs per character, back to back, bursts a few
                // character times apart
                let mut first_us = self.start.elapsed().as_micros() as u32;
                let ser = api_impl::stream_data_ser();
                for Burst {
                    line,
                    bytes,
                    measured_char_us,
                } in received
                {
                    let t = ww_si::Second::microseconds(first_us);
                    let char_us = measured_char_us.unwrap_or(CHAR_TIME_US);
                    let end_us = first_us + (bytes.len() as u32 - 1) * char_us;
                    first_us = end_us + 4 * char_us;
                    let chunk = RxChunk {
                        end: measured_char_us.map(|_| ww_si::Second::microseconds(end_us)),
                        flags: None,
                        bytes: MaxLen::new(RefVec::new_bytes(&bytes)).unwrap(),
                    };
                    let units = bytes.len() as u64;
                    let _ = match line {
                        None => {
                            ser.uart()
                                .rx_send_blocking(&mut self.rx, &chunk, units, Some(t), out)
                        }
                        Some(line) => ser.uart().monitor_send_blocking(
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

            fn uart_configure(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                config: UartConfig,
            ) -> RpcResult<Result<UartConfig, UartError<'_>>> {
                let caps = capabilities();
                if config.baud_rate > caps.max_baud_rate || config.baud_rate < caps.min_baud_rate {
                    return Ready(Err(CommonError::Unsupported.into()));
                }
                if let Some(level) = config.io_level
                    && !caps.io_levels.iter().any(|l| l == Ok(level))
                {
                    return Ready(Err(CommonError::Unsupported.into()));
                }
                let mut m = self.m.lock().unwrap();
                let io_level = config.io_level.or(m.config.io_level);
                // the divider of this mock hits every rate exactly, its idle timer counts whole characters
                let idle_chars = config.idle_chars.max(1);
                m.config = UartConfig {
                    io_level,
                    idle_chars,
                    ..config
                };
                Ready(Ok(m.config.clone()))
            }

            fn uart_set_pin_level(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                pin: Pin,
                is_high: bool,
            ) -> RpcResult<Result<(), UartError<'_>>> {
                let mut m = self.m.lock().unwrap();
                if !matches!(m.config.mode, Mode::Test) {
                    return Ready(Err(CommonError::WrongState {
                        reason: MaxLen::new_unchecked("not in Mode::Test"),
                    }
                    .into()));
                }
                m.pin_levels.push((pin, is_high));
                Ready(Ok(()))
            }

            fn uart_send_break(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                duration: ww_si::Second,
            ) -> RpcResult<Result<(), UartError<'_>>> {
                self.m.lock().unwrap().send_break_calls.push(duration);
                Ready(Ok(()))
            }

            fn uart_tx_pending(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<u32> {
                Ready(self.uart_tx_credit.buffered())
            }

            fn uart_monitor(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                config: MonitorConfig,
                // TODO(API-35): `'_` doesn't compile for a stream function's error yet
            ) -> RpcResult<Result<MonitorConfig, UartError<'static>>> {
                // this monitor decodes from 300 Bd up
                if config.baud_rate < Baud::baud(300u32) {
                    return Ready(Err(CommonError::Unsupported.into()));
                }
                let actual = MonitorConfig {
                    io_level: config.io_level.or(Some(ww_si::quantity!(3300 mV u16))),
                    ..config
                };
                self.m.lock().unwrap().monitor_config = Some(actual.clone());
                Ready(Ok(actual))
            }

            fn close_uart_monitor(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {
                self.m.lock().unwrap().monitor_config = None;
            }

            fn stream_uart_monitor(&mut self) -> &mut Counted {
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
    async fn capabilities_and_configure() {
        let (device, m) = start("io_uart/configure", Model::default());
        let client = connect(&device).await;

        let caps = client.uart().capabilities().call().await.unwrap();
        assert_eq!(caps.min_baud_rate, Baud::baud(110u32));
        assert_eq!(caps.max_baud_rate, Baud::baud(921_600u64));
        assert_eq!(caps.io_levels.len(), 2);

        let config = UartConfig {
            baud_rate: Baud::baud(9600u32),
            data_bits: DataBits::Seven,
            parity: Parity::Even,
            stop_bits: StopBits::Stop2,
            mode: Mode::HardwareFlowControl,
            prevent_back_feed: true,
            io_level: None,
            idle_chars: 0,
        };
        let actual = client
            .uart()
            .configure(config.clone())
            .call()
            .await
            .unwrap();
        // `None` keeps the IO level, the answer says which one is in effect
        assert_eq!(actual.io_level, Some(ww_si::Volt::millivolts(3300u16)));
        assert_eq!(actual.baud_rate, Baud::baud(9600u16));
        // 0 idle characters is taken as 1
        assert_eq!(actual.idle_chars, 1);
        // start + 7 data + parity + 2 stop
        assert_eq!(actual.frame_bits(), Some(11));
        assert!(matches!(actual.mode, Mode::HardwareFlowControl));
        assert!(matches!(m.lock().unwrap().config.parity, Parity::Even));

        let too_fast = UartConfig {
            baud_rate: Baud::megabaud(3u8),
            ..config.clone()
        };
        let r = client.uart().configure(too_fast).call().await;
        assert!(matches!(
            r.unwrap_err().device(),
            Some(UartErrorOwned::Common(CommonErrorOwned::Unsupported))
        ));

        let at_5v = UartConfig {
            io_level: Some(ww_si::Volt::volts(5u8)),
            ..config
        };
        let r = client.uart().configure(at_5v).call().await;
        assert!(matches!(
            r.unwrap_err().device(),
            Some(UartErrorOwned::Common(CommonErrorOwned::Unsupported))
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn set_pin_level_requires_test_mode() {
        let (device, m) = start("io_uart/pin_level", Model::default());
        let client = connect(&device).await;

        let r = client.uart().set_pin_level(Pin::Tx, true).call().await;
        assert!(matches!(
            r.unwrap_err().device(),
            Some(UartErrorOwned::Common(CommonErrorOwned::WrongState { .. }))
        ));
        assert!(m.lock().unwrap().pin_levels.is_empty());

        let test_mode = UartConfig {
            mode: Mode::Test,
            ..m.lock().unwrap().config.clone()
        };
        client.uart().configure(test_mode).call().await.unwrap();
        let r = client.uart().set_pin_level(Pin::Tx, true).call().await;
        assert!(r.is_ok());
        assert_eq!(m.lock().unwrap().pin_levels, vec![(Pin::Tx, true)]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn send_break_and_tx_pending() {
        let (device, m) = start("io_uart/break", Model::default());
        let client = connect(&device).await;

        let duration = ww_si::quantity!(10 s u32);
        client.uart().send_break(duration).call().await.unwrap();
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

        // the sender runs 2 % fast (85 µs per character instead of 87): the first burst's end is measured, the
        // second one's isn't
        m.lock()
            .unwrap()
            .received
            .push(Burst::rx(b"abcdef", Some(85)));
        m.lock().unwrap().received.push(Burst::rx(b"gh", None));
        // any request lets the mock's event loop send them
        client.uart().tx_pending().call().await.unwrap();
        let config = m.lock().unwrap().config.clone();
        let char_time_us = config.frame_bits().unwrap() as i128 * 1_000_000
            / config.baud_rate.to_i128_in(ww_si::Prefix::Unit).unwrap();
        assert_eq!(char_time_us, CHAR_TIME_US as i128 - 1); // 86.8 µs, rounded down
        for (pos, bytes, end_us) in [(0, &b"abcdef"[..], Some(5 * 85)), (6, b"gh", None)] {
            match rx.recv_event().await.unwrap() {
                StreamEvent::Item {
                    pos: p,
                    units,
                    t,
                    value,
                    ..
                } => {
                    assert_eq!((p, units), (pos, bytes.len() as u64));
                    // first byte in the head, last byte in the chunk if measured
                    let us = |t: ww_si::Second| t.to_i128_in(ww_si::Prefix::Micro).unwrap();
                    let first = us(t.expect("every chunk is timestamped"));
                    assert_eq!(value.end.map(|end| us(end) - first), end_us);
                    // not measured: one character time (frame bits / baud) per byte after the first, as the
                    // mock's line sent them (to within the rounding of the character time)
                    if value.end.is_none() {
                        let n = bytes.len() as i128 - 1;
                        let estimated = n * char_time_us;
                        assert!((n * CHAR_TIME_US as i128 - estimated).abs() <= n);
                    }
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
            io_level: None,
            idle_chars: ww_uart::IDLE_CHARS_DEFAULT,
        };
        let r = client
            .uart()
            .monitor(config(Baud::baud(110u32)))
            .call()
            .await
            .unwrap();
        assert!(
            matches!(
                r,
                Err(UartErrorOwned::Common(CommonErrorOwned::Unsupported))
            ),
            "{:?}",
            r.err()
        );

        let (actual, mut lines) = client
            .uart()
            .monitor(config(Baud::baud(115_200u32)))
            .call()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(actual.baud_rate, Baud::baud(115_200u32));
        assert_eq!(actual.io_level, Some(ww_si::Volt::millivolts(3300u16)));
        assert_eq!(
            m.lock()
                .unwrap()
                .monitor_config
                .as_ref()
                .map(|c| c.baud_rate),
            Some(Baud::baud(115_200u32))
        );

        // a request on line A, the answer on line B, in the order they arrived
        m.lock().unwrap().received.push(Burst {
            line: Some(MonitorLine::A),
            bytes: b"xyz".to_vec(),
            measured_char_us: Some(CHAR_TIME_US),
        });
        m.lock().unwrap().received.push(Burst {
            line: Some(MonitorLine::B),
            bytes: b"ok".to_vec(),
            measured_char_us: None,
        });
        client.uart().tx_pending().call().await.unwrap();
        let a = lines.recv().await.unwrap();
        assert_eq!(a.line, MonitorLine::A);
        assert_eq!(a.chunk.bytes.into_inner().as_slice(), b"xyz");
        assert!(a.chunk.end.is_some());
        let b = lines.recv().await.unwrap();
        assert_eq!(b.line, MonitorLine::B);
        assert_eq!(b.chunk.bytes.into_inner().as_slice(), b"ok");
        assert!(b.chunk.end.is_none());

        lines.close().await.unwrap();
        eventually(&m, |m| m.monitor_config.is_none()).await;
    }
}

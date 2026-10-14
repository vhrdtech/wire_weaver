//! `ww_pwm` end to end: a mock server on `ww_device::Server` through `tests_common::start_device`, the generated
//! client through the real host event loop.

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use ww_client_server::StreamSideband;
    use ww_pwm::{
        CaptureConfig, CaptureMode, CaptureSample, CaptureValue, Hertz, PwmActual, PwmCapabilities,
        PwmChannelInfo, PwmError, PwmOutput, PwmState, Second,
    };
    use ww_si::Prefix;
    use ww_si::ww_numeric::NumericValue;

    const MAX_FREQ: Hertz = Hertz {
        prefix: Prefix::Mega,
        value: NumericValue::U32(80),
    };
    const MIN_FREQ: Hertz = Hertz {
        prefix: Prefix::Unit,
        value: NumericValue::U32(1),
    };

    /// `Hertz`/`Second` don't derive `PartialEq` (their `NumericValue` carries `f32`), compare fields instead.
    fn hz_eq(a: Hertz, b: Hertz) -> bool {
        a.prefix == b.prefix && a.value == b.value
    }

    fn sec_eq(a: Second, b: Second) -> bool {
        a.prefix == b.prefix && a.value == b.value
    }

    fn capture_value_eq(a: CaptureValue, b: CaptureValue) -> bool {
        match (a, b) {
            (CaptureValue::Frequency(x), CaptureValue::Frequency(y)) => hz_eq(x, y),
            (CaptureValue::DutyCycle(x), CaptureValue::DutyCycle(y)) => x == y,
            (CaptureValue::PulseWidth(x), CaptureValue::PulseWidth(y)) => sec_eq(x, y),
            (CaptureValue::EdgeCount(x), CaptureValue::EdgeCount(y)) => x == y,
            _ => false,
        }
    }

    /// Device state, shared with the test.
    #[derive(Default)]
    struct Model {
        channels: [Option<PwmActual>; 2],
        capture_config: [Option<CaptureConfig>; 1],
        /// What `read_capture` answers once a measurement "arrives" (set directly by the test, no hardware here).
        capture_sample: [Option<CaptureSample>; 1],
        sideband: Vec<(u32, StreamSideband)>,
    }

    mod sync_server {
        use super::*;
        use tests_common::TestProcessEvents;

        pub struct Server {
            pub m: Arc<Mutex<Model>>,
        }

        impl Server {
            fn valid_indices_root_pwm_channel(&mut self) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..2)
            }

            fn pwm_capabilities(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<PwmCapabilities<'_>> {
                const CHANNELS: [PwmChannelInfo<'static>; 2] = [
                    PwmChannelInfo {
                        name: MaxLen::new_unchecked("ch0"),
                        group: 0,
                        capture: true,
                    },
                    PwmChannelInfo {
                        name: MaxLen::new_unchecked("ch1"),
                        group: 0,
                        capture: false,
                    },
                ];
                Ready(PwmCapabilities {
                    channels: MaxLen::new(RefVec::Slice { slice: &CHANNELS }).unwrap(),
                    min_freq: MIN_FREQ,
                    max_freq: MAX_FREQ,
                    duty_bits: 16,
                })
            }

            fn pwm_channel_set_output(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
                output: PwmOutput,
            ) -> RpcResult<Result<PwmActual, PwmError>> {
                let idx = index_chain[0].0 as usize;
                let actual = PwmActual {
                    freq: output.freq,
                    duty: output.duty,
                };
                self.m.lock().unwrap().channels[idx] = Some(actual);
                Ready(Ok(actual))
            }

            fn pwm_channel_set_duty(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
                duty: u16,
            ) -> RpcResult<Result<u16, PwmError>> {
                let idx = index_chain[0].0 as usize;
                let mut m = self.m.lock().unwrap();
                match &mut m.channels[idx] {
                    Some(actual) => {
                        actual.duty = duty;
                        Ready(Ok(duty))
                    }
                    None => Ready(Err(PwmError::WrongMode)),
                }
            }

            fn pwm_channel_stop(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
                _idle_high: bool,
            ) -> RpcResult<Result<(), PwmError>> {
                let idx = index_chain[0].0 as usize;
                self.m.lock().unwrap().channels[idx] = None;
                Ready(Ok(()))
            }

            fn pwm_channel_state(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
            ) -> RpcResult<PwmState> {
                let idx = index_chain[0].0 as usize;
                let state = match self.m.lock().unwrap().channels[idx] {
                    Some(actual) => PwmState::Output(actual),
                    None => PwmState::Idle,
                };
                Ready(state)
            }

            fn valid_indices_root_pwm_capture_channel(&mut self) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..1)
            }

            fn sideband_pwm_capture_channel_capture(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
                sideband: StreamSideband,
            ) -> Option<StreamSideband> {
                self.m
                    .lock()
                    .unwrap()
                    .sideband
                    .push((index_chain[0].0, sideband));
                None
            }

            fn pwm_capture_channel_set_capture(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
                config: CaptureConfig,
            ) -> RpcResult<Result<CaptureConfig, PwmError>> {
                let idx = index_chain[0].0 as usize;
                self.m.lock().unwrap().capture_config[idx] = Some(config);
                Ready(Ok(config))
            }

            fn pwm_capture_channel_read_capture(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
            ) -> RpcResult<Result<CaptureSample, PwmError>> {
                let idx = index_chain[0].0 as usize;
                match self.m.lock().unwrap().capture_sample[idx] {
                    Some(sample) => Ready(Ok(sample)),
                    None => Ready(Err(PwmError::WrongMode)),
                }
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                io_pwm_api :: IoPwmTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                // debug_to_file = "../../target/tests_io_pwm_server.rs"
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

        pub struct Client {
            pub cmd: Commander,
        }

        impl WwClient for Client {
            fn default_config() -> ClientConfig {
                ClientConfig::default()
            }

            fn from_cmd(cmd: Commander) -> Self {
                Self { cmd }
            }
        }

        mod api_client {
            wire_weaver::ww_codegen!(
                io_pwm_api :: IoPwmTest for super::Client,
                client = "std_client",
            );
        }
    }

    use client::Client;

    fn start(path: &str) -> (TestDevice, Arc<Mutex<Model>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let m = Arc::new(Mutex::new(Model::default()));
        let server = sync_server::Server { m: m.clone() };
        let device = tests_common::start_device(
            path,
            server,
            io_pwm_api::IO_PWM_TEST_FULL_GID,
            sync_server::api_impl::api_hash(),
        );
        (device, m)
    }

    async fn connect(device: &TestDevice) -> Client {
        Client::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pwm_capabilities_lists_channels() {
        let (device, _m) = start("pwm/capabilities");
        let client = connect(&device).await;
        let caps = client.pwm().capabilities().call().await.unwrap();
        assert_eq!(caps.channels.len(), 2);
        assert_eq!(caps.channels[0].name.as_str(), "ch0");
        assert!(caps.channels[0].capture);
        assert_eq!(caps.channels[1].name.as_str(), "ch1");
        assert!(!caps.channels[1].capture);
        assert_eq!(caps.duty_bits, 16);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn set_output_set_duty_and_stop() {
        let (device, m) = start("pwm/output");
        let client = connect(&device).await;
        let pwm = client.pwm();
        let mut channel = pwm.channel(0);

        let output = PwmOutput {
            freq: Hertz {
                prefix: Prefix::Kilo,
                value: NumericValue::U32(10),
            },
            duty: 32768,
            inverted: false,
            phase_correct: false,
        };
        let actual = channel.set_output(output).call().await.unwrap().unwrap();
        assert_eq!(actual.duty, 32768);
        let stored = m.lock().unwrap().channels[0].unwrap();
        assert_eq!(stored.duty, actual.duty);
        assert!(hz_eq(stored.freq, actual.freq));

        let state = channel.state().call().await.unwrap();
        match state {
            PwmState::Output(a) => {
                assert_eq!(a.duty, actual.duty);
                assert!(hz_eq(a.freq, actual.freq));
            }
            PwmState::Idle => panic!("expected Output, got Idle"),
        }

        let duty = channel.set_duty(1000).call().await.unwrap().unwrap();
        assert_eq!(duty, 1000);
        assert_eq!(m.lock().unwrap().channels[0].unwrap().duty, 1000);

        channel.stop(true).call().await.unwrap().unwrap();
        assert!(m.lock().unwrap().channels[0].is_none());
        let state = channel.state().call().await.unwrap();
        assert!(matches!(state, PwmState::Idle));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn set_duty_without_output_is_wrong_mode() {
        let (device, _m) = start("pwm/wrong_mode");
        let client = connect(&device).await;
        let pwm = client.pwm();
        let mut channel = pwm.channel(1);
        let err = channel.set_duty(100).call().await.unwrap();
        assert_eq!(err, Err(PwmError::WrongMode));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn set_capture_and_read_capture() {
        let (device, m) = start("pwm_capture/read");
        let client = connect(&device).await;
        let pwm_capture = client.pwm_capture();
        let mut channel = pwm_capture.channel(0);

        let config = CaptureConfig {
            mode: CaptureMode::Frequency,
            gate: Second {
                prefix: Prefix::Milli,
                value: NumericValue::U32(100),
            },
        };
        let actual = channel.set_capture(config).call().await.unwrap().unwrap();
        assert_eq!(actual.mode, CaptureMode::Frequency);
        let stored = m.lock().unwrap().capture_config[0].unwrap();
        assert_eq!(stored.mode, config.mode);
        assert!(sec_eq(stored.gate, config.gate));

        // no measurement taken yet
        let err = channel.read_capture().call().await.unwrap();
        assert_eq!(err.unwrap_err(), PwmError::WrongMode);

        // the device "measures" something; nothing here models real hardware, so the test sets it directly
        let sample = CaptureSample {
            t: Second {
                prefix: Prefix::Milli,
                value: NumericValue::U32(100),
            },
            value: CaptureValue::Frequency(Hertz {
                prefix: Prefix::Kilo,
                value: NumericValue::U32(5),
            }),
        };
        m.lock().unwrap().capture_sample[0] = Some(sample);
        let got = channel.read_capture().call().await.unwrap().unwrap();
        assert!(sec_eq(got.t, sample.t));
        assert!(capture_value_eq(got.value, sample.value));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn capture_stream_open_close_and_samples() {
        let (device, m) = start("pwm_capture/stream");
        let client = connect(&device).await;
        let pwm_capture = client.pwm_capture();
        let channel = pwm_capture.channel(0);

        let mut stream = channel.capture().await.unwrap();
        stream.open().await.unwrap();
        for _ in 0..200 {
            if !m.lock().unwrap().sideband.is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(m.lock().unwrap().sideband, vec![(0, StreamSideband::Open)]);

        let sample = CaptureSample {
            t: Second {
                prefix: Prefix::Milli,
                value: NumericValue::U32(1),
            },
            value: CaptureValue::DutyCycle(12345),
        };
        let mut scratch = [0u8; 64];
        device.send(
            sync_server::api_impl::stream_data_ser()
                .pwm_capture()
                .channel(0)
                .capture(&sample, &mut scratch)
                .unwrap(),
        );
        let got = stream.recv().await.unwrap();
        assert!(sec_eq(got.t, sample.t));
        assert!(capture_value_eq(got.value, sample.value));

        stream.close().await.unwrap();
        for _ in 0..200 {
            if m.lock().unwrap().sideband.len() >= 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(
            m.lock().unwrap().sideband,
            vec![(0, StreamSideband::Open), (0, StreamSideband::Close)]
        );
    }
}

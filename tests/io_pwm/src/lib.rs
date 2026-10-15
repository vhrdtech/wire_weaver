//! `ww_pwm` end to end: a mock server on `ww_device::Server` through `tests_common::start_device`, the generated
//! client through the real host event loop.

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver_client::StreamEvent;
    use ww_pwm::{
        CaptureConfig, CaptureMode, CaptureSample, CaptureValue, Hertz, PwmActual, PwmCapabilities,
        PwmChannelInfo, PwmError, PwmErrorOwned, PwmOutput, PwmState, Second,
    };
    use ww_si::Prefix;
    use ww_si::ww_numeric::NumericValue;
    use ww_stream::tx::Counted;

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
        /// What the `capture` stream of each channel measures while it is open.
        streaming: [Option<CaptureConfig>; 1],
        /// Measurements the test makes "arrive", sent on the stream before the next request.
        measured: Vec<(u32, CaptureSample)>,
    }

    mod sync_server {
        use super::*;
        use tests_common::TestProcessEvents;

        pub struct Server {
            pub m: Arc<Mutex<Model>>,
            pub capture: [Counted; 1],
            pub stream_sessions: u32,
        }

        impl Server {
            pub fn new(m: Arc<Mutex<Model>>) -> Self {
                Server {
                    m,
                    capture: [Counted::new()],
                    stream_sessions: 0,
                }
            }

            /// The event loop's producer: measurements of open `capture` streams.
            fn produce(&mut self, out: &mut impl BlockingEventOut) {
                let measured = std::mem::take(&mut self.m.lock().unwrap().measured);
                for (idx, sample) in measured {
                    let _ = api_impl::stream_data_ser()
                        .pwm_capture()
                        .channel(idx)
                        .capture_send_blocking(
                            &mut self.capture[idx as usize],
                            &sample.value,
                            1,
                            Some(sample.t),
                            out,
                        );
                }
            }

            fn new_stream_session(&mut self) -> u32 {
                self.stream_sessions = self.stream_sessions.wrapping_add(1);
                self.stream_sessions
            }

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
            ) -> RpcResult<Result<PwmActual, PwmError<'_>>> {
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
            ) -> RpcResult<Result<u16, PwmError<'_>>> {
                let idx = index_chain[0].0 as usize;
                let mut m = self.m.lock().unwrap();
                match &mut m.channels[idx] {
                    Some(actual) => {
                        actual.duty = duty;
                        Ready(Ok(duty))
                    }
                    None => Ready(Err(CommonError::WrongState {
                        reason: MaxLen::new_unchecked(""),
                    }
                    .into())),
                }
            }

            fn pwm_channel_stop(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
                _idle_high: bool,
            ) -> RpcResult<Result<(), PwmError<'_>>> {
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

            fn pwm_capture_channel_capture(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
                config: CaptureConfig,
                // TODO(API-35): `'_` doesn't compile for a stream function's error yet
            ) -> RpcResult<Result<CaptureConfig, PwmError<'static>>> {
                if config.mode == CaptureMode::Off {
                    return Ready(Err(CommonError::InvalidArgument {
                        arg: 0,
                        reason: MaxLen::new_unchecked(""),
                    }
                    .into()));
                }
                let idx = index_chain[0].0 as usize;
                self.m.lock().unwrap().streaming[idx] = Some(config);
                Ready(Ok(config))
            }

            fn close_pwm_capture_channel_capture(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
            ) {
                let idx = index_chain[0].0 as usize;
                self.m.lock().unwrap().streaming[idx] = None;
            }

            fn stream_pwm_capture_channel_capture(
                &mut self,
                index_chain: [UNib32; 1],
            ) -> &mut Counted {
                &mut self.capture[index_chain[0].0 as usize]
            }

            fn pwm_capture_channel_set_capture(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
                config: CaptureConfig,
            ) -> RpcResult<Result<CaptureConfig, PwmError<'_>>> {
                let idx = index_chain[0].0 as usize;
                self.m.lock().unwrap().capture_config[idx] = Some(config);
                Ready(Ok(config))
            }

            fn pwm_capture_channel_read_capture(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
            ) -> RpcResult<Result<CaptureSample, PwmError<'_>>> {
                let idx = index_chain[0].0 as usize;
                match self.m.lock().unwrap().capture_sample[idx] {
                    Some(sample) => Ready(Ok(sample)),
                    None => Ready(Err(CommonError::WrongState {
                        reason: MaxLen::new_unchecked(""),
                    }
                    .into())),
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
                // the mock's producer runs between requests
                self.produce(out);
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
        let server = sync_server::Server::new(m.clone());
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
        let actual = channel.set_output(output).call().await.unwrap();
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
            other => panic!("expected Output, got {other:?}"),
        }

        let duty = channel.set_duty(1000).call().await.unwrap();
        assert_eq!(duty, 1000);
        assert_eq!(m.lock().unwrap().channels[0].unwrap().duty, 1000);

        channel.stop(true).call().await.unwrap();
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
        let err = channel.set_duty(100).call().await;
        assert!(matches!(
            err.unwrap_err().device(),
            Some(PwmErrorOwned::Common(CommonErrorOwned::WrongState { .. }))
        ));
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
        let actual = channel.set_capture(config).call().await.unwrap();
        assert_eq!(actual.mode, CaptureMode::Frequency);
        let stored = m.lock().unwrap().capture_config[0].unwrap();
        assert_eq!(stored.mode, config.mode);
        assert!(sec_eq(stored.gate, config.gate));

        // no measurement taken yet
        let err = channel.read_capture().call().await;
        assert!(matches!(
            err.unwrap_err().device(),
            Some(PwmErrorOwned::Common(CommonErrorOwned::WrongState { .. }))
        ));

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
        let got = channel.read_capture().call().await.unwrap();
        assert!(sec_eq(got.t, sample.t));
        assert!(capture_value_eq(got.value, sample.value));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn capture_stream_open_close_and_samples() {
        let (device, m) = start("pwm_capture/stream");
        let client = connect(&device).await;
        let pwm_capture = client.pwm_capture();
        let mut channel = pwm_capture.channel(0);

        let config = |mode| CaptureConfig {
            mode,
            gate: Second {
                prefix: Prefix::Milli,
                value: NumericValue::U32(100),
            },
        };
        let r = channel
            .capture(config(CaptureMode::Off))
            .call()
            .await
            .unwrap();
        assert!(
            matches!(
                r,
                Err(PwmErrorOwned::Common(
                    CommonErrorOwned::InvalidArgument { .. }
                ))
            ),
            "{:?}",
            r.err()
        );

        let (actual, mut stream) = channel
            .capture(config(CaptureMode::DutyCycle))
            .call()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(actual.mode, CaptureMode::DutyCycle);
        assert_eq!(
            m.lock().unwrap().streaming[0].map(|c| c.mode),
            Some(CaptureMode::DutyCycle)
        );

        let sample = CaptureSample {
            t: Second {
                prefix: Prefix::Milli,
                value: NumericValue::U32(1),
            },
            value: CaptureValue::DutyCycle(12345),
        };
        m.lock().unwrap().measured.push((0, sample));
        // any request lets the mock's event loop send it
        channel.read_capture().call().await.ok();
        match stream.recv_event().await.unwrap() {
            StreamEvent::Item { pos, t, value, .. } => {
                assert_eq!(pos, 0);
                assert!(sec_eq(t.unwrap(), sample.t));
                assert!(capture_value_eq(value, sample.value));
            }
            ev => panic!("{ev:?}"),
        }

        stream.close().await.unwrap();
        for _ in 0..200 {
            if m.lock().unwrap().streaming[0].is_none() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(m.lock().unwrap().streaming[0].is_none());
    }
}

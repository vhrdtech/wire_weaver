//! `ww_adc` end to end: a mock server on `ww_device::Server` through `tests_common::start_device`, the generated
//! client through the real host event loop.

#[cfg(test)]
mod tests {
    use std::time::Instant;
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver_client::StreamEvent;
    use ww_adc::*;
    use ww_stream::tx::Counted;

    /// Bytes per sample of `fmt`, see [SampleFormat].
    fn sample_bytes(fmt: SampleFormat) -> usize {
        match fmt {
            SampleFormat::U8 => 1,
            SampleFormat::U16 => 2,
            SampleFormat::Packed12 => 2, // approximate, not exercised with an odd count here
            SampleFormat::I16 => 2,
            SampleFormat::I24 => 3,
            SampleFormat::Unknown(_) => {
                unreachable!("handlers reject formats outside the capabilities")
            }
        }
    }

    struct StoredStreamConfig {
        channels: Vec<u8>,
        format: SampleFormat,
    }

    #[derive(Default)]
    struct StreamSim {
        config: Option<StoredStreamConfig>,
        running: bool,
        /// A block to send from the event loop, once the open has been answered
        pending: bool,
        counted: Counted,
    }

    struct CaptureSim {
        /// Sets of the capture currently armed, triggered or done, for [CaptureStatus::Done].
        sets: u32,
        status: CaptureStatus,
        data: Vec<u8>,
        /// How many times `capture_status` was polled since the capture armed with a non-`Now` trigger; advances
        /// `Armed` -> `Triggered` -> `Done` one step per poll, standing in for the real wait on the trigger.
        polls_since_armed: u32,
    }

    impl Default for CaptureSim {
        fn default() -> Self {
            CaptureSim {
                sets: 0,
                status: CaptureStatus::Idle,
                data: Vec::new(),
                polls_since_armed: 0,
            }
        }
    }

    mod sync_server {
        use super::*;
        use tests_common::TestProcessEvents;

        /// Two channels: a calibrated 12-bit input in mV, and an uncalibrated die temperature in °C.
        fn channels() -> [ChannelInfo<'static>; 2] {
            [
                ChannelInfo {
                    name: MaxLen::new("VIN").unwrap(),
                    unit: BaseUnit::Volt,
                    prefix: Prefix::Milli,
                    min: NumericValue::F32(0.0),
                    max: NumericValue::F32(3300.0),
                    bits: 12,
                    calibrated: true,
                    raw_to_value: Some(Linear {
                        scale: 3300.0 / 4095.0,
                        offset: 0.0,
                    }),
                },
                ChannelInfo {
                    name: MaxLen::new("TEMP").unwrap(),
                    unit: BaseUnit::DegreeCelsius,
                    prefix: Prefix::Unit,
                    min: NumericValue::F32(-40.0),
                    max: NumericValue::F32(125.0),
                    bits: 12,
                    calibrated: false,
                    raw_to_value: None,
                },
            ]
        }

        pub struct Server {
            pub start: Instant,
            pub channels: [ChannelInfo<'static>; 2],
            pub stream: StreamSim,
            pub capture: CaptureSim,
            pub stream_sessions: u32,
        }

        impl Server {
            pub fn new() -> Self {
                Server {
                    start: Instant::now(),
                    channels: channels(),
                    stream: StreamSim::default(),
                    capture: CaptureSim::default(),
                    stream_sessions: 0,
                }
            }

            fn now(&self) -> Second {
                Second {
                    prefix: Prefix::Micro,
                    value: NumericValue::U32(self.start.elapsed().as_micros() as u32),
                }
            }

            fn busy(&self) -> bool {
                self.stream.running
                    || matches!(
                        self.capture.status,
                        CaptureStatus::Armed | CaptureStatus::Triggered { .. }
                    )
            }

            fn adc_read(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                channel: u8,
            ) -> RpcResult<Result<Reading, AdcError<'_>>> {
                if self.busy() {
                    return Ready(Err(CommonError::Busy.into()));
                }
                let Some(ch) = self.channels.get(channel as usize) else {
                    return Ready(Err(AdcError::NoSuchChannel));
                };
                let (raw, value) = if ch.calibrated {
                    // half scale
                    (2048u32, NumericValue::F32(2048.0 * 3300.0 / 4095.0))
                } else {
                    (0u32, NumericValue::F32(23.5))
                };
                Ready(Ok(Reading {
                    value,
                    raw,
                    t: self.now(),
                }))
            }

            fn adc_capabilities(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<AdcCapabilities<'_>> {
                Ready(AdcCapabilities {
                    channels: MaxLen::new(RefVec::Slice {
                        slice: &self.channels,
                    })
                    .unwrap(),
                })
            }

            fn adc_stream_samples(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                config: StreamConfig<'_>,
                // TODO(API-35): `'_` doesn't compile for a stream function's error yet
            ) -> RpcResult<Result<StreamActual, AdcError<'static>>> {
                if self.busy() {
                    return Ready(Err(CommonError::Busy.into()));
                }
                for &ch in config.channels.as_slice() {
                    if ch as usize >= self.channels.len() {
                        return Ready(Err(AdcError::NoSuchChannel));
                    }
                }
                if config.channels.len() as u8 > self.channels.len() as u8 {
                    return Ready(Err(CommonError::InvalidArgument {
                        arg: 0,
                        reason: MaxLen::new_unchecked(""),
                    }
                    .into()));
                }
                if !matches!(config.format, SampleFormat::U8 | SampleFormat::U16) {
                    return Ready(Err(CommonError::Unsupported.into()));
                }
                // the mock "rounds" like real hardware would: caps at 500 kHz total
                let requested =
                    numeric_as_u32(config.rate.value) * prefix_scale(config.rate.prefix);
                let capped = requested.min(500_000);
                let actual = StreamActual {
                    rate: Hertz {
                        prefix: Prefix::Unit,
                        value: NumericValue::U32(capped),
                    },
                };
                self.stream.config = Some(StoredStreamConfig {
                    channels: config.channels.as_slice().to_vec(),
                    format: config.format,
                });
                self.stream.running = true;
                // A real device starts sampling; the mock sends one block of two sets before the next request
                self.stream.pending = true;
                Ready(Ok(actual))
            }

            fn close_adc_stream_samples(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {
                self.stream.running = false;
            }

            fn stream_adc_stream_samples(&mut self) -> &mut Counted {
                &mut self.stream.counted
            }

            fn new_stream_session(&mut self) -> u32 {
                self.stream_sessions = self.stream_sessions.wrapping_add(1);
                self.stream_sessions
            }

            /// The event loop's producer: a block the open handler asked for, once the stream is open.
            fn produce(&mut self, out: &mut impl BlockingEventOut) {
                if !std::mem::take(&mut self.stream.pending) {
                    return;
                }
                let Some(config) = &self.stream.config else {
                    return;
                };
                let bytes = sample_bytes(config.format) * config.channels.len() * 2;
                let data: Vec<u8> = (0..bytes as u8).collect();
                let t = self.now();
                let _ = api_impl::stream_data_ser()
                    .adc_stream()
                    .samples_send_blocking(
                        &mut self.stream.counted,
                        &SampleBlock {
                            data: MaxLen::new(RefVec::new_bytes(&data)).unwrap(),
                        },
                        2,
                        Some(t),
                        out,
                    );
            }

            fn adc_stream_stream_capabilities(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<StreamCapabilities<'_>> {
                Ready(StreamCapabilities {
                    max_total_rate: Hertz {
                        prefix: Prefix::Kilo,
                        value: NumericValue::U32(500),
                    },
                    max_channels: self.channels.len() as u8,
                    formats: MaxLen::new(RefVec::Slice {
                        slice: &[SampleFormat::U8, SampleFormat::U16],
                    })
                    .unwrap(),
                })
            }

            fn adc_capture_capture_capabilities(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<CaptureCapabilities<'_>> {
                Ready(CaptureCapabilities {
                    max_total_rate: Hertz {
                        prefix: Prefix::Kilo,
                        value: NumericValue::U32(500),
                    },
                    max_bytes: 4096,
                    simultaneous: true,
                    level_trigger: true,
                    external_trigger: true,
                    formats: MaxLen::new(RefVec::Slice {
                        slice: &[SampleFormat::U8, SampleFormat::U16],
                    })
                    .unwrap(),
                })
            }

            fn adc_capture_capture(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                config: CaptureConfig<'_>,
            ) -> RpcResult<Result<CaptureActual, AdcError<'_>>> {
                if self.stream.running {
                    return Ready(Err(CommonError::Busy.into()));
                }
                for &ch in config.channels.as_slice() {
                    if ch as usize >= self.channels.len() {
                        return Ready(Err(AdcError::NoSuchChannel));
                    }
                }
                if !matches!(config.format, SampleFormat::U8 | SampleFormat::U16) {
                    return Ready(Err(CommonError::Unsupported.into()));
                }
                let per_set = sample_bytes(config.format) * config.channels.len();
                let bytes = per_set * config.sets as usize;
                if bytes > 4096 {
                    return Ready(Err(CommonError::InvalidArgument {
                        arg: 0,
                        reason: MaxLen::new_unchecked(""),
                    }
                    .into()));
                }
                let data: Vec<u8> = (0..bytes).map(|i| (i * 7) as u8).collect();
                let actual = CaptureActual {
                    rate: config.rate,
                    bytes: bytes as u32,
                };
                let t_first = self.now();
                let trigger = config.trigger;
                self.capture.data = data;
                self.capture.sets = config.sets;
                self.capture.polls_since_armed = 0;
                self.capture.status = match trigger {
                    Trigger::Now => CaptureStatus::Done {
                        t_first,
                        t_trigger: None,
                        sets: config.sets,
                    },
                    _ => CaptureStatus::Armed,
                };
                Ready(Ok(actual))
            }

            fn adc_capture_capture_status(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<CaptureStatus> {
                // standing in for the real wait on the trigger: one step towards Done per poll
                if let CaptureStatus::Armed = self.capture.status {
                    self.capture.polls_since_armed += 1;
                    if self.capture.polls_since_armed >= 2 {
                        self.capture.status = CaptureStatus::Triggered { t: self.now() };
                    }
                } else if let CaptureStatus::Triggered { t } = self.capture.status {
                    self.capture.polls_since_armed += 1;
                    if self.capture.polls_since_armed >= 3 {
                        self.capture.status = CaptureStatus::Done {
                            t_first: t,
                            t_trigger: Some(t),
                            sets: self.capture.sets,
                        };
                    }
                }
                Ready(self.capture.status)
            }

            fn adc_capture_capture_read(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                offset: u32,
                len: u16,
            ) -> RpcResult<Result<MaxLen<RefVec<'_, u8>, MAX_CAPTURE_READ_LEN>, AdcError<'_>>>
            {
                if !matches!(self.capture.status, CaptureStatus::Done { .. }) {
                    return Ready(Err(CommonError::WrongState {
                        reason: MaxLen::new_unchecked(""),
                    }
                    .into()));
                }
                let start = offset as usize;
                let end = start + len as usize;
                let Some(slice) = self.capture.data.get(start..end) else {
                    return Ready(Err(CommonError::InvalidArgument {
                        arg: 0,
                        reason: MaxLen::new_unchecked(""),
                    }
                    .into()));
                };
                Ready(Ok(MaxLen::new(RefVec::new_bytes(slice)).unwrap()))
            }

            fn adc_capture_capture_abort(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<()> {
                self.capture.status = CaptureStatus::Idle;
                self.capture.data.clear();
                Ready(())
            }
        }

        /// `value * 10^prefix` as an integer, good enough for the rates used in these tests.
        fn prefix_scale(prefix: Prefix) -> u32 {
            match prefix {
                Prefix::Kilo => 1_000,
                Prefix::Mega => 1_000_000,
                _ => 1,
            }
        }

        fn numeric_as_u32(value: NumericValue) -> u32 {
            match value {
                NumericValue::U8(v) => v as u32,
                NumericValue::U16(v) => v as u32,
                NumericValue::U32(v) => v,
                NumericValue::F32(v) => v as u32,
                _ => 0,
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                io_adc_api :: IoAdcTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                // debug_to_file = "../../target/tests_io_adc_server.rs"
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
                // the mock's producer runs between requests, after the open's reply went out
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
                io_adc_api :: IoAdcTest for super::Client,
                client = "std_client",
            );
        }
    }

    use client::Client;

    fn start(path: &str) -> TestDevice {
        let _ = tracing_subscriber::fmt::try_init();
        let server = sync_server::Server::new();
        tests_common::start_device(
            path,
            server,
            io_adc_api::IO_ADC_TEST_FULL_GID,
            sync_server::api_impl::api_hash(),
        )
    }

    async fn connect(device: &TestDevice) -> Client {
        Client::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn capabilities_and_read() {
        let device = start("adc/capabilities_and_read");
        let client = connect(&device).await;
        let mut adc = client.adc();

        let caps = adc.capabilities().call().await.unwrap();
        assert_eq!(caps.channels.len(), 2);
        assert_eq!(caps.channels.iter().next().unwrap().name.as_str(), "VIN");

        let reading = adc.read(0).call().await.unwrap();
        let expected = 2048.0 * 3300.0 / 4095.0;
        assert!(matches!(reading.value, NumericValue::F32(v) if (v - expected).abs() < 0.01));

        assert_eq!(
            adc.read(9).call().await.unwrap_err().device(),
            Some(&AdcErrorOwned::NoSuchChannel)
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stream_open_receive_close() {
        let device = start("adc/stream");
        let client = connect(&device).await;
        let mut adc_stream = client.adc_stream();

        let caps = adc_stream.stream_capabilities().call().await.unwrap();
        assert_eq!(caps.max_channels, 2);

        let config = |format| StreamConfigOwned {
            channels: MaxLen::new(vec![0, 1]).unwrap(),
            rate: Hertz {
                prefix: Prefix::Kilo,
                value: NumericValue::U32(1000),
            },
            format,
            flush_interval: None,
        };
        let r = adc_stream
            .samples(config(SampleFormat::I24))
            .call()
            .await
            .unwrap();
        assert!(
            matches!(r, Err(AdcErrorOwned::Common(CommonErrorOwned::Unsupported))),
            "{:?}",
            r.err()
        );

        let (actual, mut samples) = adc_stream
            .samples(config(SampleFormat::U16))
            .call()
            .await
            .unwrap()
            .unwrap();
        // the mock caps at 500 kHz
        assert_eq!(actual.rate.value, NumericValue::U32(500_000));
        // sampling runs: single readings are refused
        assert_eq!(
            client.adc().read(0).call().await.unwrap_err().device(),
            Some(&AdcErrorOwned::Common(CommonErrorOwned::Busy))
        );

        match samples.recv_event().await.unwrap() {
            StreamEvent::Item {
                pos,
                units,
                t,
                value,
                ..
            } => {
                // 2 sets of 2 channels, 2 bytes each (U16)
                assert_eq!((pos, units), (0, 2));
                assert_eq!(value.data.len(), 8);
                assert!(t.is_some());
            }
            ev => panic!("{ev:?}"),
        }

        samples.close().await.unwrap();
        // adc is free again once the stream stops
        assert!(client.adc().read(0).call().await.is_ok());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn capture_now_trigger_and_read() {
        let device = start("adc/capture_now");
        let client = connect(&device).await;
        let mut adc_capture = client.adc_capture();

        let caps = adc_capture.capture_capabilities().call().await.unwrap();
        assert!(caps.simultaneous);

        let actual = adc_capture
            .capture(CaptureConfigOwned {
                channels: MaxLen::new(vec![0]).unwrap(),
                rate: Hertz {
                    prefix: Prefix::Kilo,
                    value: NumericValue::U32(100),
                },
                sets: 4,
                pre_trigger: 0,
                trigger: Trigger::Now,
                format: SampleFormat::U8,
            })
            .call()
            .await
            .unwrap();
        assert_eq!(actual.bytes, 4);

        let status = adc_capture.capture_status().call().await.unwrap();
        assert!(matches!(status, CaptureStatus::Done { sets: 4, .. }));

        let data = adc_capture.capture_read(0, 4).call().await.unwrap();
        assert_eq!(data.as_slice(), &[0, 7, 14, 21]);

        assert!(matches!(
            adc_capture
                .capture_read(2, 10)
                .call()
                .await
                .unwrap_err()
                .device(),
            Some(AdcErrorOwned::Common(
                CommonErrorOwned::InvalidArgument { .. }
            ))
        ));

        adc_capture.capture_abort().call().await.unwrap();
        assert!(matches!(
            adc_capture
                .capture_read(0, 1)
                .call()
                .await
                .unwrap_err()
                .device(),
            Some(AdcErrorOwned::Common(CommonErrorOwned::WrongState { .. }))
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn capture_polled_trigger_progresses_to_done() {
        let device = start("adc/capture_polled");
        let client = connect(&device).await;
        let mut adc_capture = client.adc_capture();

        adc_capture
            .capture(CaptureConfigOwned {
                channels: MaxLen::new(vec![0]).unwrap(),
                rate: Hertz {
                    prefix: Prefix::Kilo,
                    value: NumericValue::U32(100),
                },
                sets: 2,
                pre_trigger: 0,
                trigger: Trigger::Rising {
                    channel: 0,
                    level: NumericValue::F32(1500.0),
                },
                format: SampleFormat::U8,
            })
            .call()
            .await
            .unwrap();

        assert!(matches!(
            adc_capture.capture_status().call().await.unwrap(),
            CaptureStatus::Armed
        ));
        assert!(matches!(
            adc_capture.capture_status().call().await.unwrap(),
            CaptureStatus::Triggered { .. }
        ));
        assert!(matches!(
            adc_capture.capture_status().call().await.unwrap(),
            CaptureStatus::Done { .. }
        ));

        // a stream can't start while a capture is armed/triggered, busy by then released
        let mut adc_stream = client.adc_stream();
        adc_stream
            .samples(StreamConfigOwned {
                channels: MaxLen::new(vec![0]).unwrap(),
                rate: Hertz {
                    prefix: Prefix::Unit,
                    value: NumericValue::U32(10),
                },
                format: SampleFormat::U8,
                flush_interval: None,
            })
            .call()
            .await
            .unwrap()
            .unwrap();
    }
}

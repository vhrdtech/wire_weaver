//! `ww_timebase` end to end: a mock server on `ww_device::Server` through `tests_common::start_device`, the generated
//! client through the real host event loop.

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use ww_si::ww_numeric::NumericValue;
    use ww_si::{Prefix, Second};
    use ww_timebase::{
        ClockError, ClockErrorOwned, ClockInfo, DateTime, SteerPoint, SyncConfig, SyncFormat,
        SyncState, SyncStatus, TimeMapPoint,
    };

    /// Where the mock's clock starts, far from the host's, so a mixed-up offset shows.
    const LOCAL_START: u32 = 1_000_000_000;

    fn micros(value: u32) -> Second {
        Second::from_parts(Prefix::Micro, NumericValue::U32(value))
    }

    fn as_micros(s: &Second) -> u32 {
        assert_eq!(s.prefix, Prefix::Micro, "{s:?}");
        match s.value {
            NumericValue::U32(v) => v,
            other => panic!("unexpected value {other:?}"),
        }
    }

    /// Device behaviour and what it received, shared with the test.
    struct Model {
        sync_config: SyncConfig,
        sync_status: SyncStatus,
        times_of_day: Vec<DateTime>,
        /// The last `steer` point taken.
        steer: Option<SteerPoint>,
        /// The `time_map` stream is open.
        time_map_open: bool,
    }

    impl Default for Model {
        fn default() -> Self {
            Model {
                sync_config: SyncConfig::Off,
                sync_status: SyncStatus {
                    state: SyncState::Free,
                    offset: None,
                    last_lock: None,
                },
                times_of_day: Vec::new(),
                steer: None,
                time_map_open: false,
            }
        }
    }

    mod sync_server {
        use super::*;
        use tests_common::TestProcessEvents;

        /// Mock clock: a real [Instant] backs `now()`, 1 % fast and starting at [LOCAL_START], the sync line only
        /// understands `SyncFormat::IrigB` and a slave locks to it right away (real hardware would stay `Acquiring`
        /// for a while); a `Host` slave is `Acquiring` until its first `steer` point.
        pub struct Server {
            pub m: Arc<Mutex<Model>>,
            pub epoch: Instant,
            pub formats: [SyncFormat; 2],
        }

        impl Server {
            fn elapsed(&self) -> Second {
                micros(LOCAL_START + (self.epoch.elapsed().as_micros() * 101 / 100) as u32)
            }

            fn send_point(
                &self,
                cx: &mut Context<'_, impl BlockingEventOut>,
                point: &TimeMapPoint,
            ) {
                api_impl::stream_data_ser()
                    .clock()
                    .time_map_send_blocking(point, cx)
                    .unwrap();
            }

            fn clock_now(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Second> {
                Ready(self.elapsed())
            }

            fn clock_info(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<ClockInfo<'_>> {
                Ready(ClockInfo {
                    resolution: micros(1),
                    source: MaxLen::new_unchecked("mock clock"),
                    sync_formats: MaxLen::new_unchecked(RefVec::Slice {
                        slice: &self.formats,
                    }),
                })
            }

            fn clock_time_map(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<()> {
                self.m.lock().unwrap().time_map_open = true;
                Ready(())
            }

            fn close_clock_time_map(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {
                self.m.lock().unwrap().time_map_open = false;
            }

            fn sideband_clock_time_map(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }

            fn clock_configure_sync(
                &mut self,
                cx: &mut Context<'_, impl BlockingEventOut>,
                config: SyncConfig,
            ) -> RpcResult<Result<(), ClockError<'_>>> {
                let format = match config {
                    SyncConfig::Off => None,
                    SyncConfig::Master { format } | SyncConfig::Slave { format } => Some(format),
                    // a config from a newer API version
                    SyncConfig::Unknown(_) => return Ready(Err(CommonError::Unsupported.into())),
                };
                if let Some(format) = format
                    && !self.formats.contains(&format)
                {
                    return Ready(Err(CommonError::Unsupported.into()));
                }
                if config
                    == (SyncConfig::Master {
                        format: SyncFormat::Host,
                    })
                {
                    // a host steers slaves, there is no line to be the master of
                    return Ready(Err(CommonError::InvalidArgument {
                        arg: 0,
                        reason: MaxLen::new_unchecked("Host is a slave format"),
                    }
                    .into()));
                }
                self.m.lock().unwrap().steer = None;
                self.m.lock().unwrap().sync_config = config;
                match config {
                    SyncConfig::Off => {
                        let local = self.elapsed();
                        self.m.lock().unwrap().sync_status = SyncStatus {
                            state: SyncState::Free,
                            offset: None,
                            last_lock: None,
                        };
                        self.send_point(
                            cx,
                            &TimeMapPoint {
                                local,
                                shared: local,
                                state: SyncState::Free,
                            },
                        );
                    }
                    SyncConfig::Master { .. } => {
                        let local = self.elapsed();
                        self.m.lock().unwrap().sync_status = SyncStatus {
                            state: SyncState::Locked,
                            offset: Some(micros(0)),
                            last_lock: Some(local),
                        };
                        self.send_point(
                            cx,
                            &TimeMapPoint {
                                local,
                                shared: local,
                                state: SyncState::Locked,
                            },
                        );
                    }
                    SyncConfig::Slave {
                        format: SyncFormat::Host,
                    } => {
                        let acquiring = self.elapsed();
                        self.m.lock().unwrap().sync_status = SyncStatus {
                            state: SyncState::Acquiring,
                            offset: None,
                            last_lock: None,
                        };
                        self.send_point(
                            cx,
                            &TimeMapPoint {
                                local: acquiring,
                                shared: acquiring,
                                state: SyncState::Acquiring,
                            },
                        );
                    }
                    SyncConfig::Slave { .. } => {
                        let acquiring = self.elapsed();
                        self.m.lock().unwrap().sync_status = SyncStatus {
                            state: SyncState::Acquiring,
                            offset: None,
                            last_lock: None,
                        };
                        self.send_point(
                            cx,
                            &TimeMapPoint {
                                local: acquiring,
                                shared: acquiring,
                                state: SyncState::Acquiring,
                            },
                        );

                        // the mock locks right away; real hardware would take a while to acquire
                        let local = self.elapsed();
                        let offset = 50;
                        let shared = micros(as_micros(&local).saturating_sub(offset));
                        self.m.lock().unwrap().sync_status = SyncStatus {
                            state: SyncState::Locked,
                            offset: Some(micros(offset)),
                            last_lock: Some(local),
                        };
                        self.send_point(
                            cx,
                            &TimeMapPoint {
                                local,
                                shared,
                                state: SyncState::Locked,
                            },
                        );
                    }
                    SyncConfig::Unknown(_) => unreachable!("rejected above"),
                }
                Ready(Ok(()))
            }

            fn clock_sync_config(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<SyncConfig> {
                Ready(self.m.lock().unwrap().sync_config)
            }

            fn clock_sync_status(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<SyncStatus> {
                Ready(self.m.lock().unwrap().sync_status)
            }

            fn clock_set_time_of_day(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                t: DateTime,
            ) -> RpcResult<Result<(), ClockError<'_>>> {
                let mut m = self.m.lock().unwrap();
                if !matches!(m.sync_config, SyncConfig::Master { .. }) {
                    return Ready(Err(CommonError::WrongState {
                        reason: MaxLen::new_unchecked("not a sync master"),
                    }
                    .into()));
                }
                m.times_of_day.push(t);
                Ready(Ok(()))
            }

            fn clock_steer(
                &mut self,
                cx: &mut Context<'_, impl BlockingEventOut>,
                point: SteerPoint,
            ) -> RpcResult<Result<(), ClockError<'_>>> {
                let mut m = self.m.lock().unwrap();
                if m.sync_config
                    != (SyncConfig::Slave {
                        format: SyncFormat::Host,
                    })
                {
                    return Ready(Err(CommonError::WrongState {
                        reason: MaxLen::new_unchecked("not a Host slave"),
                    }
                    .into()));
                }
                let (local, shared) = (as_micros(&point.local), as_micros(&point.shared));
                m.steer = Some(point);
                m.sync_status = SyncStatus {
                    state: SyncState::Locked,
                    offset: Some(Second::from_parts(
                        Prefix::Micro,
                        NumericValue::I32(local.wrapping_sub(shared) as i32),
                    )),
                    last_lock: Some(point.local),
                };
                drop(m);
                self.send_point(
                    cx,
                    &TimeMapPoint {
                        local: point.local,
                        shared: point.shared,
                        state: SyncState::Locked,
                    },
                );
                Ready(Ok(()))
            }

            fn clock_shared_now(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Option<Second>> {
                let Some(p) = self.m.lock().unwrap().steer else {
                    return Ready(None);
                };
                let dl = as_micros(&self.elapsed()).wrapping_sub(as_micros(&p.local)) as f64;
                let shared = as_micros(&p.shared) as f64 + dl * (1.0 + p.rate_ppb as f64 * 1e-9);
                Ready(Some(micros(shared.round() as u32)))
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                io_timebase_api :: IoTimebaseTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                // debug_to_file = "../../target/tests_io_timebase_server.rs"
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
                io_timebase_api :: IoTimebaseTest for super::Client,
                client = "std_client",
            );
        }
    }

    use client::Client;

    fn start(path: &str) -> (TestDevice, Arc<Mutex<Model>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let m = Arc::new(Mutex::new(Model::default()));
        let server = sync_server::Server {
            m: m.clone(),
            epoch: Instant::now(),
            formats: [SyncFormat::IrigB, SyncFormat::Host],
        };
        let device = tests_common::start_device(
            path,
            server,
            io_timebase_api::IO_TIMEBASE_TEST_FULL_GID,
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
    async fn now_increases_monotonically() {
        let (device, _m) = start("timebase/now");
        let client = connect(&device).await;
        let first = client.clock().now().call().await.unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        let second = client.clock().now().call().await.unwrap();
        assert!(as_micros(&second) > as_micros(&first));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn info_reports_resolution_and_formats() {
        let (device, _m) = start("timebase/info");
        let client = connect(&device).await;
        let info = client.clock().info().call().await.unwrap();
        assert_eq!(as_micros(&info.resolution), 1);
        assert_eq!(*info.source, "mock clock");
        assert_eq!(
            info.sync_formats.iter().copied().collect::<Vec<_>>(),
            vec![SyncFormat::IrigB, SyncFormat::Host]
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn configure_sync_master_locks_immediately_and_publishes_time_map() {
        let (device, _m) = start("timebase/master");
        let client = connect(&device).await;
        let mut time_map = client.clock().time_map().call().await.unwrap();

        client
            .clock()
            .configure_sync(SyncConfig::Master {
                format: SyncFormat::IrigB,
            })
            .call()
            .await
            .unwrap();

        let point = time_map.recv().await.unwrap();
        assert_eq!(point.state, SyncState::Locked);

        let status = client.clock().sync_status().call().await.unwrap();
        assert_eq!(status.state, SyncState::Locked);
        assert!(status.offset.is_some());
        assert!(status.last_lock.is_some());
        assert_eq!(
            client.clock().sync_config().call().await.unwrap(),
            SyncConfig::Master {
                format: SyncFormat::IrigB
            }
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn configure_sync_slave_acquires_then_locks() {
        let (device, _m) = start("timebase/slave");
        let client = connect(&device).await;
        let mut time_map = client.clock().time_map().call().await.unwrap();

        client
            .clock()
            .configure_sync(SyncConfig::Slave {
                format: SyncFormat::IrigB,
            })
            .call()
            .await
            .unwrap();

        assert_eq!(time_map.recv().await.unwrap().state, SyncState::Acquiring);
        assert_eq!(time_map.recv().await.unwrap().state, SyncState::Locked);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn configure_sync_rejects_unsupported_format() {
        let (device, _m) = start("timebase/unsupported");
        let client = connect(&device).await;
        let result = client
            .clock()
            .configure_sync(SyncConfig::Master {
                format: SyncFormat::Pps,
            })
            .call()
            .await;
        assert_eq!(
            result.unwrap_err().device(),
            Some(&ClockErrorOwned::Common(CommonErrorOwned::Unsupported))
        );
        assert_eq!(
            client.clock().sync_status().call().await.unwrap().state,
            SyncState::Free
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn set_time_of_day_requires_master() {
        let (device, _m) = start("timebase/time_of_day");
        let client = connect(&device).await;
        let t = DateTime::from_ymd_hms_utc_opt(2026, 10, 14, 12, 0, 0, 0).unwrap();

        let result = client.clock().set_time_of_day(t).call().await;
        assert!(matches!(
            result.unwrap_err().device(),
            Some(ClockErrorOwned::Common(CommonErrorOwned::WrongState { .. }))
        ));

        client
            .clock()
            .configure_sync(SyncConfig::Master {
                format: SyncFormat::IrigB,
            })
            .call()
            .await
            .unwrap();
        client.clock().set_time_of_day(t).call().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn host_sync_steers_the_device_onto_host_time() {
        use ww_timebase::host::{DeviceTime, Estimator, EstimatorConfig, Sample};
        let (device, m) = start("timebase/host");
        let client = connect(&device).await;
        let mut time_map = client.clock().time_map().call().await.unwrap();
        let host_epoch = Instant::now();
        // shared time: the host's clock, far from the device's own
        let host_ns = || host_epoch.elapsed().as_nanos() as u64 + 77_000_000_000;

        // steering before configuring is refused
        let point = SteerPoint {
            local: micros(0),
            shared: micros(0),
            rate_ppb: 0,
        };
        let result = client.clock().steer(point).call().await;
        assert!(matches!(
            result.unwrap_err().device(),
            Some(ClockErrorOwned::Common(CommonErrorOwned::WrongState { .. }))
        ));
        assert_eq!(client.clock().shared_now().call().await.unwrap(), None);
        let host_master = client
            .clock()
            .configure_sync(SyncConfig::Master {
                format: SyncFormat::Host,
            })
            .call()
            .await;
        assert!(matches!(
            host_master.unwrap_err().device(),
            Some(ClockErrorOwned::Common(
                CommonErrorOwned::InvalidArgument { .. }
            ))
        ));

        let host_slave = SyncConfig::Slave {
            format: SyncFormat::Host,
        };
        client
            .clock()
            .configure_sync(host_slave)
            .call()
            .await
            .unwrap();
        assert_eq!(time_map.recv().await.unwrap().state, SyncState::Acquiring);

        // a short window for the test: in-process round trips are tens of µs
        let mut estimator = Estimator::new(EstimatorConfig {
            min_span: Duration::from_millis(40),
            ..EstimatorConfig::default()
        });
        let mut dt = None;
        let fit = loop {
            let sent = host_ns();
            let now = client.clock().now().call().await.unwrap();
            let received = host_ns();
            let dt = dt.get_or_insert_with(|| DeviceTime::new(&now).unwrap());
            estimator.add(Sample {
                sent,
                device: dt.to_ns(&now).unwrap(),
                received,
            });
            if let Some(fit) = estimator.fit() {
                break fit;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        };
        // the mock runs 1 % fast: within a few hundred ppm after 40 ms
        assert!(
            (fit.rate_ppb() + 9_900_990).abs() < 1_000_000,
            "{}",
            fit.rate_ppb()
        );
        let dt = dt.unwrap();
        let point = dt.steer_point(&fit, fit.local).unwrap();
        client.clock().steer(point).call().await.unwrap();
        assert_eq!(m.lock().unwrap().steer.unwrap().rate_ppb, fit.rate_ppb());
        let locked = time_map.recv().await.unwrap();
        assert_eq!(locked.state, SyncState::Locked);
        assert_eq!(as_micros(&locked.shared), as_micros(&point.shared));
        assert_eq!(
            client.clock().sync_status().call().await.unwrap().state,
            SyncState::Locked
        );

        // the device now tells host time; the bound only catches unit, sign and offset mix-ups, not timing
        let before = host_ns() / 1000;
        let shared = client.clock().shared_now().call().await.unwrap().unwrap();
        let after = host_ns() / 1000;
        let shared = as_micros(&shared) as u64;
        assert!(
            shared + 50_000 > before && shared < after + 50_000,
            "{before} <= {shared} <= {after}"
        );
    }
}

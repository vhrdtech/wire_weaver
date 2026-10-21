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
        SyncSource, SyncState, SyncStatus, TimeMapPoint,
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
        /// The last `steer` point followed.
        steer: Option<SteerPoint>,
        /// `steer` points taken, followed or only noted.
        steers: u32,
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
                    source: None,
                    error: None,
                },
                times_of_day: Vec::new(),
                steer: None,
                steers: 0,
                time_map_open: false,
            }
        }
    }

    mod sync_server {
        use super::*;
        use tests_common::TestProcessEvents;

        /// Mock clock: a real [Instant] backs `now()`, 1 % fast and starting at [LOCAL_START]. The sync line only
        /// understands `SyncFormat::IrigB` and a slave locks to it right away (real hardware would stay `Acquiring`
        /// for a while); `steer` points are followed unless the line is locked, as the ranking of sources says.
        pub struct Server {
            pub m: Arc<Mutex<Model>>,
            pub epoch: Instant,
            pub formats: [SyncFormat; 1],
            /// The list of `set_sources`, `None`: the fixed ranking.
            pub sources: Option<Vec<SyncSource>>,
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
                self.m.lock().unwrap().sync_config = config;
                match config {
                    SyncConfig::Off => {
                        let local = self.elapsed();
                        self.m.lock().unwrap().sync_status = SyncStatus {
                            state: SyncState::Free,
                            offset: None,
                            last_lock: None,
                            source: None,
                            error: None,
                        };
                        self.send_point(
                            cx,
                            &TimeMapPoint {
                                local,
                                shared: local,
                                state: SyncState::Free,
                                source: None,
                            },
                        );
                    }
                    SyncConfig::Master { .. } => {
                        let local = self.elapsed();
                        self.m.lock().unwrap().sync_status = SyncStatus {
                            state: SyncState::Locked,
                            offset: Some(micros(0)),
                            last_lock: Some(local),
                            source: None,
                            error: None,
                        };
                        self.send_point(
                            cx,
                            &TimeMapPoint {
                                local,
                                shared: local,
                                state: SyncState::Locked,
                                source: None,
                            },
                        );
                    }
                    SyncConfig::Slave { .. } => {
                        let acquiring = self.elapsed();
                        self.m.lock().unwrap().sync_status = SyncStatus {
                            state: SyncState::Acquiring,
                            offset: None,
                            last_lock: None,
                            source: None,
                            error: None,
                        };
                        self.send_point(
                            cx,
                            &TimeMapPoint {
                                local: acquiring,
                                shared: acquiring,
                                state: SyncState::Acquiring,
                                source: None,
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
                            source: Some(SyncSource::Line {
                                format: SyncFormat::IrigB,
                            }),
                            error: None,
                        };
                        self.send_point(
                            cx,
                            &TimeMapPoint {
                                local,
                                shared,
                                state: SyncState::Locked,
                                source: Some(SyncSource::Line {
                                    format: SyncFormat::IrigB,
                                }),
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
            ) -> RpcResult<Result<Option<SyncSource>, ClockError<'_>>> {
                if point.local.prefix != Prefix::Micro
                    || !matches!(point.local.value, NumericValue::U32(_))
                {
                    return Ready(Err(CommonError::InvalidArgument {
                        arg: 0,
                        reason: MaxLen::new_unchecked("local not in U32 µs"),
                    }
                    .into()));
                }
                let mut m = self.m.lock().unwrap();
                m.steers += 1;
                let line = SyncSource::Line {
                    format: SyncFormat::IrigB,
                };
                let line_locked = matches!(m.sync_config, SyncConfig::Slave { .. })
                    && m.sync_status.state == SyncState::Locked;
                // best first: the list's order, or the fixed ranking (a line before the host)
                let rank = |s: SyncSource| match &self.sources {
                    Some(list) => list.iter().position(|l| *l == s),
                    None => Some(if s == SyncSource::Host { 1 } else { 0 }),
                };
                let host = rank(SyncSource::Host);
                if host.is_none() || (line_locked && rank(line) < host) {
                    // only noted
                    return Ready(Ok(line_locked.then_some(line)));
                }
                m.steer = Some(point);
                m.sync_status = SyncStatus {
                    state: SyncState::Locked,
                    offset: None,
                    last_lock: Some(point.local),
                    source: Some(SyncSource::Host),
                    error: Some(micros(point.error_us)),
                };
                drop(m);
                self.send_point(
                    cx,
                    &TimeMapPoint {
                        local: point.local,
                        shared: micros(point.shared_us as u32),
                        state: SyncState::Locked,
                        source: Some(SyncSource::Host),
                    },
                );
                Ready(Ok(Some(SyncSource::Host)))
            }

            fn clock_set_sources(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                sources: Option<MaxLen<RefVec<'_, SyncSource>, 8>>,
            ) -> RpcResult<Result<(), ClockError<'_>>> {
                let Some(sources) = sources else {
                    self.sources = None;
                    return Ready(Ok(()));
                };
                let mut list = Vec::new();
                for s in sources.iter() {
                    let s = s.unwrap();
                    if let SyncSource::Line { format } = s
                        && !self.formats.contains(&format)
                    {
                        return Ready(Err(CommonError::Unsupported.into()));
                    }
                    if list.contains(&s) {
                        return Ready(Err(CommonError::InvalidArgument {
                            arg: 0,
                            reason: MaxLen::new_unchecked("a source named twice"),
                        }
                        .into()));
                    }
                    list.push(s);
                }
                self.sources = Some(list);
                Ready(Ok(()))
            }

            fn clock_sources(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Option<MaxLen<RefVec<'_, SyncSource>, 8>>> {
                Ready(self.sources.as_ref().map(|list| {
                    MaxLen::new_unchecked(RefVec::Slice {
                        slice: list.as_slice(),
                    })
                }))
            }

            fn clock_shared_now(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Option<u64>> {
                let Some(p) = self.m.lock().unwrap().steer else {
                    return Ready(None);
                };
                let dl = as_micros(&self.elapsed()).wrapping_sub(as_micros(&p.local)) as f64;
                let shared = p.shared_us as f64 + dl * (1.0 + p.rate_ppb as f64 * 1e-9);
                Ready(Some(shared.round() as u64))
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
            formats: [SyncFormat::IrigB],
            sources: None,
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
            vec![SyncFormat::IrigB]
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
    async fn host_sync_steers_the_device_onto_utc() {
        use ww_clock_sync::{ClockSync, EstimatorConfig, Options};
        let (device, m) = start("timebase/host");
        let client = connect(&device).await;
        let mut time_map = client.clock().time_map().call().await.unwrap();
        assert_eq!(client.clock().shared_now().call().await.unwrap(), None);

        // fast, for the test: in-process round trips are tens of µs
        let options = Options {
            poll: Duration::from_millis(2),
            steer_every: Duration::from_millis(20),
            check_every: Some(Duration::from_millis(20)),
            estimator: EstimatorConfig {
                min_span: Duration::from_millis(40),
                ..EstimatorConfig::default()
            },
        };
        let sync = ClockSync::start(&client.cmd, options).await.unwrap();
        let mut watch = sync.watch();
        let status = tokio::time::timeout(
            Duration::from_secs(2),
            watch.wait_for(|s| s.residual.is_some() && s.device.is_some()),
        )
        .await
        .expect("a check within 2 s")
        .unwrap()
        .clone();
        // the mock runs 1 % fast: the fit sees it within a few hundred ppm after 40 ms
        let fit = status.fit.unwrap();
        assert!(
            (fit.rate_ppb() + 9_900_990).abs() < 1_000_000,
            "{}",
            fit.rate_ppb()
        );
        assert_eq!(status.source, Some(SyncSource::Host));
        let device_status = status.device.unwrap();
        assert_eq!(device_status.state, SyncState::Locked);
        assert_eq!(device_status.source, Some(SyncSource::Host));
        // the device tells UTC: the bound only catches unit, sign and offset mix-ups, not timing
        let residual = status.residual.unwrap();
        assert!(residual.abs() < 50_000_000, "{residual} ns");
        let point = time_map.recv().await.unwrap();
        assert_eq!(point.state, SyncState::Locked);
        assert_eq!(point.source, Some(SyncSource::Host));
        assert!(m.lock().unwrap().steers > 0);
        sync.stop();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_locked_line_beats_the_host() {
        let (device, m) = start("timebase/line_beats_host");
        let client = connect(&device).await;
        client
            .clock()
            .configure_sync(SyncConfig::Slave {
                format: SyncFormat::IrigB,
            })
            .call()
            .await
            .unwrap();
        let point = SteerPoint {
            local: micros(LOCAL_START),
            shared_us: 1_700_000_000_000_000,
            rate_ppb: 0,
            error_us: 100,
        };
        let source = client.clock().steer(point).call().await.unwrap();
        assert_eq!(
            source,
            Some(SyncSource::Line {
                format: SyncFormat::IrigB
            })
        );
        // noted, not followed
        assert!(m.lock().unwrap().steer.is_none());
        assert_eq!(client.clock().shared_now().call().await.unwrap(), None);
        // a point in a storage the device doesn't read
        let bad = SteerPoint {
            local: Second::micro(5u64),
            ..point
        };
        let result = client.clock().steer(bad).call().await;
        assert!(matches!(
            result.unwrap_err().device(),
            Some(ClockErrorOwned::Common(
                CommonErrorOwned::InvalidArgument { .. }
            ))
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_source_list_decides() {
        let (device, m) = start("timebase/sources");
        let client = connect(&device).await;
        assert_eq!(client.clock().sources().call().await.unwrap(), None);
        let point = SteerPoint {
            local: micros(LOCAL_START),
            shared_us: 1_700_000_000_000_000,
            rate_ppb: 0,
            error_us: 100,
        };
        let irig = SyncSource::Line {
            format: SyncFormat::IrigB,
        };
        // "never let a PC steer me": noted, nothing followed
        client
            .clock()
            .set_sources(Some(MaxLen::new_unchecked(vec![irig])))
            .call()
            .await
            .unwrap();
        assert_eq!(
            client.clock().sources().call().await.unwrap(),
            Some(MaxLen::new_unchecked(vec![irig]))
        );
        assert_eq!(client.clock().steer(point).call().await.unwrap(), None);
        assert!(m.lock().unwrap().steer.is_none());

        // the host before the line: followed even while the line is locked
        client
            .clock()
            .configure_sync(SyncConfig::Slave {
                format: SyncFormat::IrigB,
            })
            .call()
            .await
            .unwrap();
        client
            .clock()
            .set_sources(Some(MaxLen::new_unchecked(vec![SyncSource::Host, irig])))
            .call()
            .await
            .unwrap();
        assert_eq!(
            client.clock().steer(point).call().await.unwrap(),
            Some(SyncSource::Host)
        );

        // back to the fixed ranking: the locked line wins
        client.clock().set_sources(None).call().await.unwrap();
        client
            .clock()
            .configure_sync(SyncConfig::Slave {
                format: SyncFormat::IrigB,
            })
            .call()
            .await
            .unwrap();
        assert_eq!(
            client.clock().steer(point).call().await.unwrap(),
            Some(irig)
        );

        // a line the device doesn't have, a source twice
        let pps = SyncSource::Line {
            format: SyncFormat::Pps,
        };
        let r = client
            .clock()
            .set_sources(Some(MaxLen::new_unchecked(vec![pps])))
            .call()
            .await;
        assert_eq!(
            r.unwrap_err().device(),
            Some(&ClockErrorOwned::Common(CommonErrorOwned::Unsupported))
        );
        let r = client
            .clock()
            .set_sources(Some(MaxLen::new_unchecked(vec![
                SyncSource::Host,
                SyncSource::Host,
            ])))
            .call()
            .await;
        assert!(matches!(
            r.unwrap_err().device(),
            Some(ClockErrorOwned::Common(
                CommonErrorOwned::InvalidArgument { .. }
            ))
        ));
    }
}

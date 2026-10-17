//! `ww_can_bus` groups end to end: a mock of two FDCAN-like controllers sharing a clock (`CanGroup`, every
//! optional item of `Can` left out), time-triggered operation and CAN XL on controller 0.

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use ww_can_bus::{
        BitTiming, CanConfig, CanError, CanErrorOwned, CanId, Capabilities, ClockSource,
        ErrorState, FdConfig, FrameKind, GroupCapabilities, GroupClock, GroupState, Mode, Options,
        Performance, RxOverflow, Status, Tdc, Timing, TimingLimits, Trigger, TriggerKind, TtConfig,
        TtLevel, TtRole, TtStatus, TtSync, TxOrder, XlConfig,
    };
    use ww_si::{Baud, Hertz, Second};

    /// Clock sources of the mock: an 80 MHz PLL output and a 25 MHz crystal, divided by 1..=30 (FDCAN `CKDIV`).
    const SOURCES: [(&str, u32); 2] = [("PLL1Q", 80_000_000), ("HSE", 25_000_000)];

    #[derive(Default)]
    struct Controller {
        config: Option<CanConfig>,
        status: Option<Status>,
        tt: Option<TtConfig>,
        schedule: Vec<Trigger>,
        xl: Option<XlConfig>,
        /// Frames sent, `(id, data)`
        sent: Vec<(CanId, Vec<u8>)>,
        /// XL frames sent, `(priority, data)`
        sent_xl: Vec<(u16, Vec<u8>)>,
    }

    impl Controller {
        fn mode(&self) -> Mode {
            self.config.as_ref().map(|c| c.mode).unwrap_or(Mode::Off)
        }
    }

    enum Pending {
        Rx(usize, CanId, Vec<u8>),
        RxXl(usize, u16, Vec<u8>),
        GroupState,
    }

    struct Model {
        clock: GroupClock,
        enabled: bool,
        controllers: [Controller; 2],
        pending: Vec<Pending>,
    }

    impl Default for Model {
        fn default() -> Self {
            Model {
                clock: GroupClock {
                    source: 0,
                    frequency: Hertz::mega(80u8),
                },
                enabled: false,
                controllers: Default::default(),
                pending: vec![],
            }
        }
    }

    fn hz(f: Hertz) -> u32 {
        f.to_i128_in(ww_si::Prefix::Unit).unwrap() as u32
    }

    impl Model {
        fn group_state(&self) -> GroupState {
            if self.enabled {
                GroupState::Enabled(self.clock)
            } else {
                GroupState::Disabled
            }
        }

        /// Segments for a rate at the group's clock, prescaler 1..=4, or `None`.
        fn segments(&self, t: Timing) -> Option<Timing> {
            let Timing::Rate { rate, sample_point } = t else {
                return Some(t);
            };
            let clock = hz(self.clock.frequency);
            let rate = rate.to_i128_in(ww_si::Prefix::Unit)? as u32;
            (1..=4u16).find_map(|prescaler| {
                let quanta = clock / prescaler as u32 / rate;
                (quanta * prescaler as u32 * rate == clock && (8..=385).contains(&quanta)).then(
                    || {
                        let sample = (quanta * sample_point as u32 + 500) / 1000;
                        Timing::Segments(BitTiming {
                            prescaler,
                            seg1: (sample - 1) as u16,
                            seg2: (quanta - sample) as u8,
                            sjw: 1,
                        })
                    },
                )
            })
        }
    }

    fn timing_limits() -> TimingLimits {
        TimingLimits {
            prescaler_min: 1,
            prescaler_max: 512,
            seg1_min: 2,
            seg1_max: 256,
            seg2_min: 1,
            seg2_max: 128,
            sjw_max: 128,
        }
    }

    fn invalid(reason: &'static str) -> CanError<'static> {
        CommonError::InvalidArgument {
            arg: 0,
            reason: MaxLen::new_unchecked(reason),
        }
        .into()
    }

    fn wrong_state(reason: &'static str) -> CanError<'static> {
        CommonError::WrongState {
            reason: MaxLen::new_unchecked(reason),
        }
        .into()
    }

    mod server {
        use super::*;
        use tests_common::TestProcessEvents;
        use ww_client_server::credit::SinkCredit;
        use ww_stream::tx::Counted;

        pub struct Server {
            pub m: Arc<Mutex<Model>>,
            pub rx: [Counted; 2],
            pub tx: [SinkCredit; 2],
            pub rx_xl: Counted,
            pub tx_xl: SinkCredit,
            pub stream_sessions: u32,
            /// Clock source names, as `ClockSource` borrows them
            pub names: [&'static str; 2],
        }

        fn idx(chain: [UNib32; 1]) -> usize {
            chain[0].0 as usize
        }

        impl Server {
            fn produce(&mut self, out: &mut impl BlockingEventOut) {
                let (pending, state) = {
                    let mut m = self.m.lock().unwrap();
                    (std::mem::take(&mut m.pending), m.group_state())
                };
                let ser = api_impl::stream_data_ser();
                let t = Some(Second::micro(1000u16));
                for p in pending {
                    match p {
                        Pending::Rx(i, id, data) => {
                            let frame = ww_can_bus::RxFrame {
                                frame: ww_can_bus::Frame {
                                    id,
                                    kind: FrameKind::Data,
                                    data: MaxLen::new(RefVec::new_bytes(&data)).unwrap(),
                                },
                                filter: None,
                            };
                            let _ = ser.can().interface(i as u32).rx_send_blocking(
                                &mut self.rx[i],
                                &frame,
                                1,
                                t,
                                out,
                            );
                        }
                        Pending::RxXl(i, priority, data) => {
                            let frame = ww_can_bus::XlFrame {
                                priority: U11::new(priority).unwrap(),
                                vcid: None,
                                sdt: 0x05,
                                sec: false,
                                af: 0x1234,
                                data: MaxLen::new(RefVec::new_bytes(&data)).unwrap(),
                            };
                            let _ = ser.can_xl(i as u32).rx_xl_send_blocking(
                                &mut self.rx_xl,
                                &frame,
                                1,
                                t,
                                out,
                            );
                        }
                        Pending::GroupState => {
                            let _ = ser.can().group_state_send_blocking(&state, out);
                        }
                    }
                }
            }

            fn new_stream_session(&mut self) -> u32 {
                self.stream_sessions = self.stream_sessions.wrapping_add(1);
                self.stream_sessions
            }

            fn valid_indices_root_can_interface(&mut self) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..2)
            }

            /// Only controller 0 has TTCAN and CAN XL (as FDCAN1 of an STM32H7 has TTCAN).
            fn valid_indices_root_can_tt(&mut self) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..1)
            }

            fn valid_indices_root_can_xl(&mut self) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..1)
            }

            fn can_group_capabilities(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<GroupCapabilities<'_>> {
                let sources: &'static [ClockSource<'static>] = Box::leak(
                    SOURCES
                        .iter()
                        .zip(self.names)
                        .map(|((_, f), name)| ClockSource {
                            name: MaxLen::new_unchecked(name),
                            min: Hertz::base(f / 30),
                            max: Hertz::base(*f),
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                );
                Ready(GroupCapabilities {
                    interfaces: 2,
                    clock_sources: MaxLen::new(RefVec::Slice { slice: sources }).unwrap(),
                    default_clock: Model::default().clock,
                    shared_memory: Some(2560),
                })
            }

            fn can_enable(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Result<GroupClock, CanError<'_>>> {
                let mut m = self.m.lock().unwrap();
                if m.enabled {
                    return Ready(Err(wrong_state("enabled")));
                }
                m.enabled = true;
                m.pending.push(Pending::GroupState);
                Ready(Ok(m.clock))
            }

            fn can_disable(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Result<(), CanError<'_>>> {
                let mut m = self.m.lock().unwrap();
                if m.controllers.iter().any(|c| c.mode() != Mode::Off) {
                    return Ready(Err(wrong_state("a controller is not Off")));
                }
                m.enabled = false;
                m.pending.push(Pending::GroupState);
                Ready(Ok(()))
            }

            fn get_can_group_state(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<GroupState, ()> {
                Value(self.m.lock().unwrap().group_state())
            }

            fn can_set_clock(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                clock: GroupClock,
            ) -> RpcResult<Result<GroupClock, CanError<'_>>> {
                let mut m = self.m.lock().unwrap();
                if m.enabled {
                    return Ready(Err(wrong_state("enabled")));
                }
                let Some((_, source_hz)) = SOURCES.get(clock.source as usize) else {
                    return Ready(Err(CommonError::Unsupported.into()));
                };
                // the nearest divider not above the frequency asked for
                let div = source_hz.div_ceil(hz(clock.frequency)).clamp(1, 30);
                m.clock = GroupClock {
                    source: clock.source,
                    frequency: Hertz::base(source_hz / div),
                };
                Ready(Ok(m.clock))
            }

            fn can_performance(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Performance> {
                Ready(Performance {
                    min_frames_per_second: Some(20_000),
                    min_bytes_per_second: Some(500_000),
                    independent_of_system: true,
                })
            }

            fn can_interface_capabilities(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _index_chain: [UNib32; 1],
            ) -> RpcResult<Capabilities<'_>> {
                let clock = self.m.lock().unwrap().clock.frequency;
                Ready(Capabilities {
                    clock,
                    nominal: timing_limits(),
                    data: Some(timing_limits()),
                    tdc: true,
                    restricted: true,
                    listen_only: true,
                    internal_loopback: true,
                    external_loopback: true,
                    test_mode: false,
                    sleep: false,
                    one_shot: true,
                    presume_ack: false,
                    triple_sample: false,
                    tx_pause: true,
                    tx_order: true,
                    rx_overflow: true,
                    edge_filtering: true,
                    protocol_exception: true,
                    wake_on_bus: false,
                    long_dlc: false,
                    auto_recovery: true,
                    timestamps: None,
                    rx_fifos: 2,
                    rx_fifo_depth: 64,
                    tx_queue_depth: 32,
                    tx_buffers: 0,
                    rx_buffers: 0,
                    standard_filters: 0,
                    extended_filters: 0,
                    filter_mask: false,
                    filter_range: false,
                    memory: None,
                    transceiver_modes: ww_can_bus::TransceiverModes {
                        standby: false,
                        silent: false,
                        sleep: false,
                    },
                    termination: MaxLen::new(RefVec::Slice { slice: &[] }).unwrap(),
                    performance: None,
                })
            }

            fn can_interface_configure(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
                config: CanConfig,
            ) -> RpcResult<Result<CanConfig, CanError<'_>>> {
                let mut m = self.m.lock().unwrap();
                if !m.enabled {
                    return Ready(Err(wrong_state("group disabled")));
                }
                let Some(nominal) = m.segments(config.nominal) else {
                    return Ready(Err(invalid("bit rate not reachable")));
                };
                let fd = match config.fd {
                    Some(fd) => match m.segments(fd.data) {
                        Some(data) => Some(FdConfig { data, ..fd }),
                        None => return Ready(Err(invalid("data bit rate not reachable"))),
                    },
                    None => None,
                };
                let actual = CanConfig {
                    nominal,
                    fd,
                    ..config
                };
                let c = &mut m.controllers[idx(index_chain)];
                c.config = Some(actual.clone());
                c.status = Some(Status {
                    state: if actual.mode == Mode::Off {
                        ErrorState::Stopped
                    } else {
                        ErrorState::Active
                    },
                    tec: 0,
                    rec: 0,
                    last_error: None,
                    last_data_error: None,
                });
                Ready(Ok(actual))
            }

            fn can_interface_config(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
            ) -> RpcResult<CanConfig> {
                let m = self.m.lock().unwrap();
                Ready(
                    m.controllers[idx(index_chain)]
                        .config
                        .clone()
                        .unwrap_or_else(default_config),
                )
            }

            fn get_can_interface_status(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
            ) -> GetResult<Status, ()> {
                let m = self.m.lock().unwrap();
                Value(m.controllers[idx(index_chain)].status.unwrap_or(Status {
                    state: ErrorState::Stopped,
                    tec: 0,
                    rec: 0,
                    last_error: None,
                    last_data_error: None,
                }))
            }

            fn can_interface_rx(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _index_chain: [UNib32; 1],
            ) -> RpcResult<()> {
                Ready(())
            }

            fn close_can_interface_rx(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _index_chain: [UNib32; 1],
            ) {
            }

            fn stream_can_interface_rx(&mut self, index_chain: [UNib32; 1]) -> &mut Counted {
                &mut self.rx[idx(index_chain)]
            }

            fn credit_can_interface_tx(&mut self, index_chain: [UNib32; 1]) -> &mut SinkCredit {
                &mut self.tx[idx(index_chain)]
            }

            fn sideband_can_interface_tx(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _index_chain: [UNib32; 1],
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }

            /// Internal loopback: back on the same controller's `rx` only.
            fn write_can_interface_tx(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
                value: ww_can_bus::TxFrame<'_>,
            ) {
                let i = idx(index_chain);
                let data = value.frame.data.as_slice().to_vec();
                let mut m = self.m.lock().unwrap();
                if m.controllers[i].mode() == Mode::InternalLoopback {
                    m.pending.push(Pending::Rx(i, value.frame.id, data.clone()));
                }
                m.controllers[i].sent.push((value.frame.id, data));
                let len = self.tx[i].last_item_len();
                self.tx[i].free(len);
            }

            fn can_tt_configure_tt(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
                config: Option<TtConfig>,
            ) -> RpcResult<Result<Option<TtConfig>, CanError<'_>>> {
                let mut m = self.m.lock().unwrap();
                let c = &mut m.controllers[idx(index_chain)];
                if !matches!(c.mode(), Mode::Off | Mode::Config) {
                    return Ready(Err(wrong_state("not in Mode::Config")));
                }
                if let Some(tt) = config
                    && !tt.matrix_cycles.is_power_of_two()
                {
                    return Ready(Err(invalid("matrix_cycles")));
                }
                c.tt = config;
                Ready(Ok(config))
            }

            fn can_tt_set_schedule(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
                triggers: MaxLen<RefVec<'_, Trigger>, 64>,
            ) -> RpcResult<Result<(), CanError<'_>>> {
                let list: Vec<Trigger> = triggers.iter().map(|t| t.unwrap()).collect();
                // trigger memory of the mock: 8 elements
                if list.len() > 8 {
                    return Ready(Err(CanError::OutOfMemory { available: 8 }));
                }
                if !list.is_sorted_by_key(|t| t.time_mark) {
                    return Ready(Err(invalid("unsorted")));
                }
                self.m.lock().unwrap().controllers[idx(index_chain)].schedule = list;
                Ready(Ok(()))
            }

            fn get_can_tt_tt_status(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
            ) -> GetResult<TtStatus, ()> {
                let m = self.m.lock().unwrap();
                let c = &m.controllers[idx(index_chain)];
                let running = c.tt.is_some() && c.mode() == Mode::Normal;
                Value(TtStatus {
                    sync: if running {
                        TtSync::InSchedule
                    } else {
                        TtSync::Off
                    },
                    is_master: running
                        && matches!(c.tt.map(|t| t.role), Some(TtRole::Master { .. })),
                    cycle: 0,
                    error_level: 0,
                })
            }

            fn can_xl_configure_xl(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
                config: Option<XlConfig>,
            ) -> RpcResult<Result<Option<XlConfig>, CanError<'_>>> {
                let mut m = self.m.lock().unwrap();
                let i = idx(index_chain);
                if config.is_some()
                    && m.controllers[i]
                        .config
                        .as_ref()
                        .and_then(|c| c.fd)
                        .is_none()
                {
                    return Ready(Err(wrong_state("CAN FD not configured")));
                }
                let actual = match config {
                    Some(xl) => match m.segments(xl.data) {
                        Some(data) => Some(XlConfig { data, ..xl }),
                        None => return Ready(Err(invalid("XL bit rate not reachable"))),
                    },
                    None => None,
                };
                m.controllers[i].xl = actual;
                Ready(Ok(actual))
            }

            fn can_xl_rx_xl(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _index_chain: [UNib32; 1],
            ) -> RpcResult<()> {
                Ready(())
            }

            fn close_can_xl_rx_xl(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _index_chain: [UNib32; 1],
            ) {
            }

            fn stream_can_xl_rx_xl(&mut self, _index_chain: [UNib32; 1]) -> &mut Counted {
                &mut self.rx_xl
            }

            fn credit_can_xl_tx_xl(&mut self, _index_chain: [UNib32; 1]) -> &mut SinkCredit {
                &mut self.tx_xl
            }

            fn sideband_can_xl_tx_xl(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _index_chain: [UNib32; 1],
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }

            fn write_can_xl_tx_xl(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
                value: ww_can_bus::XlTxFrame<'_>,
            ) {
                let i = idx(index_chain);
                let data = value.frame.data.as_slice().to_vec();
                let priority = value.frame.priority.value();
                let mut m = self.m.lock().unwrap();
                if m.controllers[i].xl.is_some()
                    && m.controllers[i].mode() == Mode::InternalLoopback
                {
                    m.pending.push(Pending::RxXl(i, priority, data.clone()));
                }
                m.controllers[i].sent_xl.push((priority, data));
                let len = self.tx_xl.last_item_len();
                self.tx_xl.free(len);
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                io_can_group_api :: IoCanGroupTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                omit = [
                    "can.interface.set_mode", "can.interface.recover", "can.interface.tx_abort",
                    "can.interface.tx_events", "can.interface.bus_errors", "can.interface.protocol_status",
                    "can.interface.bus_load", "can.interface.detect_timing", "can.interface.filters",
                    "can.interface.set_layout", "can.interface.rx_buffers", "can.interface.tx_buffers",
                    "can.interface.transceiver", "can.interface.terminator", "can.interface.test_pins"
                ],
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
                io_can_group_api :: IoCanGroupTest for super::StdClient,
                client = "std_client",
            );
        }
    }

    use client::StdClient;

    fn default_config() -> CanConfig {
        CanConfig {
            mode: Mode::Off,
            nominal: rate(500, 875),
            fd: None,
            options: Options {
                one_shot: false,
                presume_ack: false,
                triple_sample: false,
                tx_pause: false,
                tx_order: TxOrder::Fifo,
                rx_overflow: RxOverflow::KeepOldest,
                edge_filtering: false,
                protocol_exception: true,
                wake_on_bus: false,
            },
            bus_off_recovery: None,
        }
    }

    fn rate(kbit: u16, sample_point: u16) -> Timing {
        Timing::Rate {
            rate: Baud::kilo(kbit),
            sample_point,
        }
    }

    fn start(path: &str) -> (TestDevice, Arc<Mutex<Model>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let m = Arc::new(Mutex::new(Model::default()));
        let credit = || ww_client_server::credit::SinkCredit::new(32 * 72);
        let server = server::Server {
            m: m.clone(),
            rx: [ww_stream::tx::Counted::new(), ww_stream::tx::Counted::new()],
            tx: [credit(), credit()],
            rx_xl: ww_stream::tx::Counted::new(),
            // two XL frames of 2 KiB
            tx_xl: ww_client_server::credit::SinkCredit::new(2 * 2100),
            stream_sessions: 0,
            names: [SOURCES[0].0, SOURCES[1].0],
        };
        let device = tests_common::start_device(
            path,
            server,
            io_can_group_api::IO_CAN_GROUP_TEST_FULL_GID,
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

    async fn eventually(m: &Arc<Mutex<Model>>, f: impl Fn(&Model) -> bool) {
        for _ in 0..500 {
            if f(&m.lock().unwrap()) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        panic!("condition not met within 500ms");
    }

    fn is_wrong_state<T: std::fmt::Debug>(
        r: Result<T, wire_weaver_client::CallError<CanErrorOwned>>,
    ) -> bool {
        matches!(
            r.unwrap_err().device(),
            Some(CanErrorOwned::Common(CommonErrorOwned::WrongState { .. }))
        )
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn clock_enable_disable() {
        let (device, _m) = start("io_can_group/clock");
        let client = connect(&device).await;
        let mut state = client.can().observe_group_state().await.unwrap();

        let caps = client.can().group_capabilities().call().await.unwrap();
        assert_eq!(caps.interfaces, 2);
        assert_eq!(caps.clock_sources.len(), 2);
        assert_eq!(caps.clock_sources[1].name.as_str(), "HSE");
        assert_eq!(
            client.can().read_group_state().read().await.unwrap(),
            GroupState::Disabled
        );
        // a controller can't be configured while its group's clock is off
        assert!(is_wrong_state(
            client
                .can()
                .interface(1)
                .configure(default_config())
                .call()
                .await
        ));

        // a custom clock: 40 MHz from the PLL (divider 2)
        let clock = client
            .can()
            .set_clock(GroupClock {
                source: 0,
                frequency: Hertz::mega(40u8),
            })
            .call()
            .await
            .unwrap();
        assert_eq!(hz(clock.frequency), 40_000_000);
        let enabled = client.can().enable().call().await.unwrap();
        assert_eq!(enabled, clock);
        // any request lets the mock's event loop send the change
        client.can().read_group_state().read().await.unwrap();
        assert_eq!(state.recv().await.unwrap(), GroupState::Enabled(clock));
        assert!(is_wrong_state(client.can().set_clock(clock).call().await));
        // the controllers count their time quanta in the group's clock
        assert_eq!(
            hz(client
                .can()
                .interface(0)
                .capabilities()
                .call()
                .await
                .unwrap()
                .clock),
            40_000_000
        );

        let actual = client
            .can()
            .interface(1)
            .configure(CanConfig {
                mode: Mode::Normal,
                ..default_config()
            })
            .call()
            .await
            .unwrap();
        // 40 MHz / 500 kbit/s = 80 quanta
        assert_eq!(actual.nominal.segments().unwrap().quanta(), 80);
        // the group stays clocked while a controller runs
        assert!(is_wrong_state(client.can().disable().call().await));
        client
            .can()
            .interface(1)
            .configure(default_config())
            .call()
            .await
            .unwrap();
        client.can().disable().call().await.unwrap();
        client.can().read_group_state().read().await.unwrap();
        assert_eq!(state.recv().await.unwrap(), GroupState::Disabled);

        let perf = client.can().performance().call().await.unwrap();
        assert_eq!(perf.min_frames_per_second, Some(20_000));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn interfaces_are_independent() {
        let (device, m) = start("io_can_group/interfaces");
        let client = connect(&device).await;
        client.can().enable().call().await.unwrap();
        for (i, kbit) in [(0, 500), (1, 1000)] {
            client
                .can()
                .interface(i)
                .configure(CanConfig {
                    mode: Mode::InternalLoopback,
                    nominal: rate(kbit, 800),
                    ..default_config()
                })
                .call()
                .await
                .unwrap();
        }
        let mut rx0 = client.can().interface(0).rx().call().await.unwrap();
        let mut rx1 = client.can().interface(1).rx().call().await.unwrap();
        let mut tx1 = client.can().interface(1).tx().await.unwrap();
        let frame = ww_can_bus::TxFrameOwned {
            frame: ww_can_bus::FrameOwned {
                id: CanId::standard(0x55).unwrap(),
                kind: FrameKind::Data,
                data: MaxLen::new(vec![1, 2]).unwrap(),
            },
            marker: None,
        };
        tx1.send_owned(frame).await.unwrap();
        tx1.flush().await.unwrap();
        eventually(&m, |m| m.controllers[1].sent.len() == 1).await;
        client.can().interface(0).config().call().await.unwrap();
        let got = rx1.recv().await.unwrap();
        assert_eq!(got.frame.id, CanId::standard(0x55).unwrap());
        assert!(m.lock().unwrap().controllers[0].sent.is_empty());
        // nothing on controller 0
        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx0.recv())
                .await
                .is_err()
        );
        // the optional items of `Can` are left out on this device
        let r = client
            .can()
            .interface(0)
            .set_mode(Mode::Config)
            .call()
            .await;
        assert!(matches!(
            r,
            Err(wire_weaver_client::CallError::Unimplemented { .. })
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn time_triggered() {
        let (device, m) = start("io_can_group/tt");
        let client = connect(&device).await;
        client.can().enable().call().await.unwrap();
        let tt = TtConfig {
            level: TtLevel::Level1,
            role: TtRole::Master { priority: 0 },
            reference_id: CanId::standard(0x080).unwrap(),
            matrix_cycles: 4,
            ntu_numerator: 0x10000,
            ntu_denominator: 0x1000,
            watchdog: None,
        };
        client
            .can()
            .interface(0)
            .configure(CanConfig {
                mode: Mode::Config,
                ..default_config()
            })
            .call()
            .await
            .unwrap();
        assert_eq!(
            client
                .can_tt(0)
                .configure_tt(Some(tt))
                .call()
                .await
                .unwrap(),
            Some(tt)
        );
        let trigger = |time_mark, kind| Trigger {
            time_mark,
            repeat: 1,
            offset: 0,
            kind,
        };
        let schedule = vec![
            trigger(0, TriggerKind::TxReference),
            trigger(100, TriggerKind::TxExclusive { buffer: 0 }),
            trigger(200, TriggerKind::RxCheck { filter: 1 }),
            trigger(900, TriggerKind::EndOfList),
        ];
        client
            .can_tt(0)
            .set_schedule(MaxLen::new(schedule.clone()).unwrap())
            .call()
            .await
            .unwrap();
        assert_eq!(m.lock().unwrap().controllers[0].schedule, schedule);
        let r = client
            .can_tt(0)
            .set_schedule(MaxLen::new(vec![trigger(0, TriggerKind::Watch); 9]).unwrap())
            .call()
            .await;
        assert!(matches!(
            r.unwrap_err().device(),
            Some(CanErrorOwned::OutOfMemory { available: 8 })
        ));

        client
            .can()
            .interface(0)
            .configure(CanConfig {
                mode: Mode::Normal,
                ..default_config()
            })
            .call()
            .await
            .unwrap();
        let status = client.can_tt(0).read_tt_status().read().await.unwrap();
        assert_eq!((status.sync, status.is_master), (TtSync::InSchedule, true));
        // changed only in configuration mode
        assert!(is_wrong_state(
            client.can_tt(0).configure_tt(None).call().await
        ));
        // controller 1 has no TTCAN
        assert!(client.can_tt(1).read_tt_status().read().await.is_err());
    }

    /// Fits the 1024-byte messages of `tests_common`; a full 2048-byte XL frame needs a device that takes messages of
    /// about 2.1 KiB.
    const XL_LEN: u32 = 900;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn xl_frames() {
        let (device, m) = start("io_can_group/xl");
        let client = connect(&device).await;
        client.can().enable().call().await.unwrap();
        let xl = XlConfig {
            data: rate(10_000, 700),
            tdc: Tdc::Auto,
            pwm: None,
            error_signaling: true,
        };
        client
            .can()
            .interface(0)
            .configure(CanConfig {
                mode: Mode::InternalLoopback,
                ..default_config()
            })
            .call()
            .await
            .unwrap();
        // XL needs FD
        assert!(is_wrong_state(
            client.can_xl(0).configure_xl(Some(xl)).call().await
        ));
        client
            .can()
            .interface(0)
            .configure(CanConfig {
                mode: Mode::InternalLoopback,
                fd: Some(FdConfig {
                    data: rate(2000, 750),
                    tdc: Tdc::Auto,
                    iso: true,
                }),
                ..default_config()
            })
            .call()
            .await
            .unwrap();
        let actual = client
            .can_xl(0)
            .configure_xl(Some(xl))
            .call()
            .await
            .unwrap()
            .unwrap();
        // 80 MHz / 10 Mbit/s = 8 quanta
        assert_eq!(actual.data.segments().unwrap().quanta(), 8);

        let mut rx = client.can_xl(0).rx_xl().call().await.unwrap();
        let mut tx = client.can_xl(0).tx_xl().await.unwrap();
        let data: Vec<u8> = (0..XL_LEN).map(|i| i as u8).collect();
        tx.send_owned(ww_can_bus::XlTxFrameOwned {
            frame: ww_can_bus::XlFrameOwned {
                priority: U11::new(0x123).unwrap(),
                vcid: Some(7),
                sdt: 0x05,
                sec: false,
                af: 0xCAFE,
                data: MaxLen::new(data.clone()).unwrap(),
            },
            marker: Some(UNib32(1)),
        })
        .await
        .unwrap();
        tx.flush().await.unwrap();
        eventually(&m, |m| m.controllers[0].sent_xl.len() == 1).await;
        client.can().interface(0).config().call().await.unwrap();
        let got = rx.recv().await.unwrap();
        assert_eq!(got.priority.value(), 0x123);
        assert_eq!(got.data.as_slice(), data.as_slice());
    }
}

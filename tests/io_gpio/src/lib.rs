//! `ww_gpio` end to end: a mock server on `ww_device::Server` through `tests_common::start_device`, the generated
//! client through the real host event loop, see `docs/api/traits.md` and `docs/std_library/ww_gpio.md`.

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver_client::StreamEvent;
    use ww_gpio::{
        BankCapabilities, BankConfig, GpioError, GpioErrorOwned, IoPinEnabledEvents, IoPinEvent,
        Level, Mode, PinCapabilities, Pull, Speed, Volt,
    };
    use ww_stream::tx::Counted;

    /// One bank of the mock device, named like a real MCU port.
    struct BankModel {
        name: &'static str,
        reference_voltage: Volt,
        pins: Vec<PinModel>,
    }

    /// State of one mock pin, mirrored by the test's assertions.
    #[derive(Clone, Copy)]
    struct PinModel {
        mode: Mode,
        output_level: Level,
        pull: Pull,
        speed: Speed,
    }

    impl Default for PinModel {
        fn default() -> Self {
            PinModel {
                mode: Mode::Input,
                output_level: Level::Low,
                pull: Pull::None,
                speed: Speed::Medium,
            }
        }
    }

    fn mock_banks() -> Vec<BankModel> {
        vec![
            BankModel {
                name: "PA",
                reference_voltage: ww_si::Volt::milli(3300u16),
                pins: vec![PinModel::default(); 4],
            },
            BankModel {
                name: "PB",
                reference_voltage: ww_si::Volt::milli(1800u16),
                pins: vec![PinModel::default(); 2],
            },
        ]
    }

    mod server {
        use super::*;
        use tests_common::TestProcessEvents;
        use wire_weaver::BlockingEventOut;

        /// The `event` stream of one pin: which edges it reports while open.
        #[derive(Default)]
        pub struct PinEvents {
            pub counted: Counted,
            pub rising: bool,
            pub falling: bool,
        }

        pub struct Server {
            pub banks: Arc<Mutex<Vec<BankModel>>>,
            pub events: Vec<Vec<PinEvents>>,
            /// Edges seen by the mock's loopback, sent before the next request (after the reply that caused them)
            pub pending: Vec<([UNib32; 2], IoPinEvent)>,
            pub stream_sessions: u32,
            pub start: std::time::Instant,
        }

        impl Server {
            pub fn new(banks: Arc<Mutex<Vec<BankModel>>>) -> Self {
                let events = banks
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|b| b.pins.iter().map(|_| PinEvents::default()).collect())
                    .collect();
                Server {
                    banks,
                    events,
                    pending: Vec::new(),
                    stream_sessions: 0,
                    start: std::time::Instant::now(),
                }
            }

            /// Loopback: the input follows the output, an edge goes to the pin's `event` stream if it reports it.
            fn edge(&mut self, index_chain: [UNib32; 2], before: Level, after: Level) {
                let (bank, pin) = Self::idx(index_chain);
                let events = &self.events[bank][pin];
                let ev = match (before, after) {
                    (Level::Low, Level::High) if events.rising => IoPinEvent::RisingEdge,
                    (Level::High, Level::Low) if events.falling => IoPinEvent::FallingEdge,
                    _ => return,
                };
                self.pending.push((index_chain, ev));
            }

            /// The event loop's producer: what the edges left for the `event` streams.
            fn produce(&mut self, out: &mut impl BlockingEventOut) {
                let t = ww_si::Second::micro(self.start.elapsed().as_micros() as u32);
                for (index_chain, ev) in std::mem::take(&mut self.pending) {
                    let (bank, pin) = Self::idx(index_chain);
                    let _ = api_impl::stream_data_ser()
                        .bank(bank as u32)
                        .pin(pin as u32)
                        .event_send_blocking(
                            &mut self.events[bank][pin].counted,
                            &ev,
                            1,
                            Some(t),
                            out,
                        );
                }
            }

            fn new_stream_session(&mut self) -> u32 {
                self.stream_sessions = self.stream_sessions.wrapping_add(1);
                self.stream_sessions
            }

            fn valid_indices_root_bank(&mut self) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..self.banks.lock().unwrap().len() as u32)
            }

            fn valid_indices_root_bank_pin(
                &mut self,
                index_chain: [UNib32; 1],
            ) -> ValidIndices<'_> {
                let bank = index_chain[0].0 as usize;
                ValidIndices::range_u32(0..self.banks.lock().unwrap()[bank].pins.len() as u32)
            }

            fn bank_capabilities(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _index_chain: [UNib32; 1],
            ) -> RpcResult<BankCapabilities<'_>> {
                Ready(BankCapabilities {
                    io_levels: MaxLen::new(RefVec::Slice {
                        slice: &const {
                            [
                                ww_si::Volt::from_parts(
                                    ww_si::Prefix::Milli,
                                    ww_si::ww_numeric::NumericValue::U16(3300),
                                ),
                                ww_si::Volt::from_parts(
                                    ww_si::Prefix::Milli,
                                    ww_si::ww_numeric::NumericValue::U16(1800),
                                ),
                            ]
                        },
                    })
                    .unwrap(),
                    io_level_min: None,
                    io_level_max: None,
                    push_pull: true,
                    open_drain: true,
                    input: true,
                    individually_configurable_pins: true,
                    custom_mode: Default::default(),
                    custom_pull: Default::default(),
                    custom_speed: Default::default(),
                })
            }

            fn bank_configure(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
                config: BankConfig,
            ) -> RpcResult<Result<BankConfig, GpioError<'_>>> {
                let bank = index_chain[0].0 as usize;
                let mut banks = self.banks.lock().unwrap();
                if let Some(level) = config.io_level {
                    let supported = [ww_si::Volt::milli(3300u16), ww_si::Volt::milli(1800u16)];
                    if !supported.contains(&level) {
                        return Ready(Err(CommonError::Unsupported.into()));
                    }
                    banks[bank].reference_voltage = level;
                }
                Ready(Ok(BankConfig {
                    io_level: Some(banks[bank].reference_voltage),
                }))
            }

            fn bank_name(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
            ) -> RpcResult<&'_ str> {
                let bank = index_chain[0].0 as usize;
                Ready(self.banks.lock().unwrap()[bank].name)
            }

            fn bank_pin_set_output_level(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 2],
                level: Level,
            ) -> RpcResult<()> {
                let before = self.with_pin_mut(index_chain, |pin| {
                    std::mem::replace(&mut pin.output_level, level)
                });
                self.edge(index_chain, before, level);
                Ready(())
            }

            fn bank_pin_toggle(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 2],
            ) -> RpcResult<()> {
                let before = self.with_pin_mut(index_chain, |pin| {
                    let before = pin.output_level;
                    pin.output_level = if before.is_high() {
                        Level::Low
                    } else {
                        Level::High
                    };
                    before
                });
                let after = self.pin(index_chain).output_level;
                self.edge(index_chain, before, after);
                Ready(())
            }

            fn bank_pin_output_level(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 2],
            ) -> RpcResult<Level> {
                Ready(self.pin(index_chain).output_level)
            }

            fn bank_pin_input_level(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 2],
            ) -> RpcResult<Level> {
                // Loopback: the mock has no real pads, input mirrors the output register.
                Ready(self.pin(index_chain).output_level)
            }

            fn bank_pin_event(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 2],
                enabled: IoPinEnabledEvents<'_>,
            ) -> RpcResult<Result<(), GpioError<'_>>> {
                if !enabled.custom.is_empty() {
                    // The mock pins have no device-specific sources.
                    return Ready(Err(CommonError::Unsupported.into()));
                }
                let (bank, pin) = Self::idx(index_chain);
                let events = &mut self.events[bank][pin];
                events.rising = enabled.rising;
                events.falling = enabled.falling;
                Ready(Ok(()))
            }

            fn close_bank_pin_event(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 2],
            ) {
                let (bank, pin) = Self::idx(index_chain);
                let events = &mut self.events[bank][pin];
                events.rising = false;
                events.falling = false;
            }

            fn stream_bank_pin_event(&mut self, index_chain: [UNib32; 2]) -> &mut Counted {
                let (bank, pin) = Self::idx(index_chain);
                &mut self.events[bank][pin].counted
            }

            fn bank_pin_set_mode(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 2],
                mode: Mode,
                initial: Option<Level>,
            ) -> RpcResult<Result<(), GpioError<'_>>> {
                if matches!(mode, Mode::Analog) {
                    // The mock pins don't support Analog, exercised by `set_mode_unsupported`.
                    return Ready(Err(CommonError::Unsupported.into()));
                }
                self.with_pin_mut(index_chain, |pin| {
                    pin.mode = mode;
                    if let Some(level) = initial {
                        pin.output_level = level;
                    }
                });
                Ready(Ok(()))
            }

            fn bank_pin_mode(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 2],
            ) -> RpcResult<Mode> {
                Ready(self.pin(index_chain).mode)
            }

            fn set_bank_pin_pull(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 2],
                value: Pull,
            ) -> SetResult<GpioError<'_>> {
                self.with_pin_mut(index_chain, |pin| pin.pull = value);
                Set
            }

            fn get_bank_pin_pull(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 2],
            ) -> GetResult<Pull, GpioError<'_>> {
                Value(self.pin(index_chain).pull)
            }

            fn set_bank_pin_speed(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 2],
                value: Speed,
            ) -> SetResult<GpioError<'_>> {
                self.with_pin_mut(index_chain, |pin| pin.speed = value);
                Set
            }

            fn get_bank_pin_speed(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 2],
            ) -> GetResult<Speed, GpioError<'_>> {
                Value(self.pin(index_chain).speed)
            }

            fn bank_pin_capabilities(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _index_chain: [UNib32; 2],
            ) -> RpcResult<PinCapabilities<'_>> {
                Ready(PinCapabilities {
                    modes: MaxLen::new_unchecked(RefVec::Slice {
                        slice: &[Mode::PushPullOutput, Mode::OpenDrainOutput, Mode::Input],
                    }),
                    pulls: MaxLen::new_unchecked(RefVec::Slice {
                        slice: &[Pull::None, Pull::Up, Pull::Down],
                    }),
                    speeds: MaxLen::new_unchecked(RefVec::Slice {
                        slice: &[Speed::Slow, Speed::Medium, Speed::Fast],
                    }),
                    events: true,
                    name: MaxLen::new_unchecked("pin"),
                })
            }

            fn idx(index_chain: [UNib32; 2]) -> (usize, usize) {
                (index_chain[0].0 as usize, index_chain[1].0 as usize)
            }

            fn pin(&self, index_chain: [UNib32; 2]) -> PinModel {
                let (bank, pin) = Self::idx(index_chain);
                self.banks.lock().unwrap()[bank].pins[pin]
            }

            fn with_pin_mut<R>(
                &mut self,
                index_chain: [UNib32; 2],
                f: impl FnOnce(&mut PinModel) -> R,
            ) -> R {
                let (bank, pin) = Self::idx(index_chain);
                f(&mut self.banks.lock().unwrap()[bank].pins[pin])
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                io_gpio_api :: IoGpioTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                // debug_to_file = "../../target/tests_io_gpio_server.rs"
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
                // the mock's producer runs between requests, after the reply that caused the edges went out
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
                io_gpio_api :: IoGpioTest for super::StdClient,
                client = "std_client",
                // debug_to_file = "../../target/tests_io_gpio_client.rs"
            );
        }
    }

    use std_client::StdClient;

    fn start(path: &str) -> (TestDevice, Arc<Mutex<Vec<BankModel>>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let banks = Arc::new(Mutex::new(mock_banks()));
        let srv = server::Server::new(banks.clone());
        let device = tests_common::start_device(
            path,
            srv,
            io_gpio_api::IO_GPIO_TEST_FULL_GID,
            server::api_impl::api_hash(),
        );
        (device, banks)
    }

    async fn connect(device: &TestDevice) -> StdClient {
        StdClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bank_resources() {
        let (device, _banks) = start("io_gpio/bank_resources");
        let client = connect(&device).await;

        let banks = client.bank_valid_indices().read().await.unwrap();
        assert_eq!(banks.iter().collect::<Vec<_>>(), vec![0, 1]);

        assert_eq!(client.bank(0).name().call().await.unwrap(), "PA");
        assert_eq!(client.bank(1).name().call().await.unwrap(), "PB");

        let caps = client.bank(0).capabilities().call().await.unwrap();
        assert!(caps.push_pull);
        assert!(caps.open_drain);
        assert!(caps.input);
        assert!(caps.individually_configurable_pins);

        // `None` changes nothing and answers the level in effect
        let actual = client
            .bank(0)
            .configure(BankConfig { io_level: None })
            .call()
            .await
            .unwrap();
        assert_eq!(actual.io_level, Some(ww_si::Volt::milli(3300u16)));

        let actual = client
            .bank(0)
            .configure(BankConfig {
                io_level: Some(ww_si::Volt::milli(1800u16)),
            })
            .call()
            .await
            .unwrap();
        assert_eq!(actual.io_level, Some(ww_si::Volt::milli(1800u32)));

        let err = client
            .bank(0)
            .configure(BankConfig {
                io_level: Some(ww_si::Volt::base(5u8)),
            })
            .call()
            .await
            .unwrap_err();
        assert!(matches!(
            err.device(),
            Some(GpioErrorOwned::Common(CommonErrorOwned::Unsupported))
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pin_levels_and_toggle() {
        let (device, _banks) = start("io_gpio/pin_levels_and_toggle");
        let client = connect(&device).await;

        let bank = client.bank(0);
        let pins = bank.pin_valid_indices().read().await.unwrap();
        assert_eq!(pins.iter().count(), 4);

        let mut pin = bank.pin(2);
        assert_eq!(pin.output_level().call().await.unwrap(), Level::Low);

        pin.set_output_level(Level::High).call().await.unwrap();
        assert_eq!(pin.output_level().call().await.unwrap(), Level::High);
        assert_eq!(pin.input_level().call().await.unwrap(), Level::High);

        pin.toggle().call().await.unwrap();
        assert_eq!(pin.output_level().call().await.unwrap(), Level::Low);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pin_mode_and_properties() {
        let (device, _banks) = start("io_gpio/pin_mode_and_properties");
        let client = connect(&device).await;
        let bank = client.bank(0);
        let mut pin = bank.pin(0);

        assert_eq!(pin.mode().call().await.unwrap(), Mode::Input);
        pin.set_mode(Mode::PushPullOutput, Some(Level::High))
            .call()
            .await
            .unwrap();
        assert_eq!(pin.mode().call().await.unwrap(), Mode::PushPullOutput);
        assert_eq!(pin.output_level().call().await.unwrap(), Level::High);

        let err = pin.set_mode(Mode::Analog, None).call().await;
        assert!(matches!(
            err.unwrap_err().device(),
            Some(GpioErrorOwned::Common(CommonErrorOwned::Unsupported))
        ));

        pin.write_pull(Pull::Up).write().await.unwrap();
        assert_eq!(pin.read_pull().read().await.unwrap(), Pull::Up);

        pin.write_speed(Speed::Fast).write().await.unwrap();
        assert_eq!(pin.read_speed().read().await.unwrap(), Speed::Fast);

        let caps = pin.capabilities().call().await.unwrap();
        assert!(caps.events);
        assert!(caps.modes.contains(&Mode::PushPullOutput));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pin_event_stream() {
        let (device, _banks) = start("io_gpio/pin_event_stream");
        let client = connect(&device).await;
        let bank = client.bank(1);
        let mut pin = bank.pin(1);

        let r = pin
            .event(ww_gpio::IoPinEnabledEventsOwned {
                rising: true,
                falling: false,
                custom: vec![3],
            })
            .call()
            .await
            .unwrap();
        assert!(
            matches!(
                r,
                Err(GpioErrorOwned::Common(CommonErrorOwned::Unsupported))
            ),
            "{:?}",
            r.err()
        );

        let ((), mut events) = pin
            .event(ww_gpio::IoPinEnabledEventsOwned {
                rising: true,
                falling: false,
                custom: Default::default(),
            })
            .call()
            .await
            .unwrap()
            .unwrap();

        // loopback: rising, falling (not reported), rising; each edge goes out before the next request
        pin.toggle().call().await.unwrap();
        pin.toggle().call().await.unwrap();
        pin.toggle().call().await.unwrap();
        pin.output_level().call().await.unwrap();
        for expect_pos in [0, 1] {
            match events.recv_event().await.unwrap() {
                StreamEvent::Item { pos, t, value, .. } => {
                    assert_eq!(pos, expect_pos);
                    assert_eq!(value, IoPinEvent::RisingEdge);
                    assert!(t.is_some());
                }
                ev => panic!("{ev:?}"),
            }
        }
        events.close().await.unwrap();
    }

    /// What `ww pins tui` does on a device with GPIO banks and no PinDiag: every pin a point, found through the
    /// introspection data, driven and read through `ww_pin_probe::live`, modes and pulls put back on release.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pins_view_over_banks() {
        use wire_weaver_client::{ClientConfig, DynClient};
        use ww_pin_probe::bank::BankPins;
        use ww_pin_probe::live::{Command, Live};

        let (device, banks) = start("io_gpio/pins_view_over_banks");
        banks.lock().unwrap()[1].pins[1].pull = Pull::Down;
        let client = DynClient::from_config(ClientConfig::new().in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");
        let (mut pins, points) = BankPins::find(&client).await.unwrap();
        // every mock pin calls itself "pin": named by bank and index instead
        let names: Vec<(&str, &str)> = points
            .iter()
            .map(|p| (p.name.as_str(), p.group.as_str()))
            .collect();
        assert_eq!(
            names,
            [
                ("PA0", "PA"),
                ("PA1", "PA"),
                ("PA2", "PA"),
                ("PA3", "PA"),
                ("PB0", "PB"),
                ("PB1", "PB")
            ]
        );
        assert!(points[0].caps.drive_high && points[0].caps.pull_up && !points[0].caps.weak_drive);

        let mut live = Live::new(points);
        live.read_all(&mut pins).await.unwrap();
        assert_eq!(live.state[2].level, Some(Level::Low));
        live.command(&mut pins, Some(2), Command::Drive(Level::High))
            .await
            .unwrap();
        // the mock loops its output back to its input
        assert_eq!(live.state[2].level, Some(Level::High));
        assert_eq!(banks.lock().unwrap()[0].pins[2].mode, Mode::PushPullOutput);
        live.command(&mut pins, Some(5), Command::Input(Pull::Up))
            .await
            .unwrap();
        assert_eq!(banks.lock().unwrap()[1].pins[1].pull, Pull::Up);

        live.command(&mut pins, None, Command::Release)
            .await
            .unwrap();
        let b = banks.lock().unwrap();
        assert_eq!(b[0].pins[2].mode, Mode::Input);
        assert_eq!(b[1].pins[1].pull, Pull::Down);
    }
}

//! `ww_pin_diag` end to end: a mock board on `ww_device::Server` through `tests_common::start_device`, the generated
//! client through the real host event loop, and `ww_pin_probe` (what `ww pins` runs) through a `DynClient` that finds
//! `PinDiag` in the device's introspection data.

#[cfg(test)]
mod tests {
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use ww_pin_diag::{
        Drive, InternedOwned, Level, PinDiagErrorOwned, Point, PointCaps, PointState, Pull,
    };

    const ALL: PointCaps = PointCaps {
        read: true,
        pull_up: true,
        pull_down: true,
        drive_low: true,
        drive_high: true,
        weak_drive: true,
    };
    const INPUT_ONLY: PointCaps = PointCaps {
        drive_low: false,
        drive_high: false,
        weak_drive: false,
        ..ALL
    };
    const DRIVE_ONLY: PointCaps = PointCaps {
        read: false,
        pull_up: false,
        pull_down: false,
        drive_low: true,
        drive_high: true,
        weak_drive: false,
    };

    // point ids of the mock board
    const P0: u16 = 0; // shorted to P1
    const P1: u16 = 1;
    const P2: u16 = 2; // external pull-up
    const DET: u16 = 3; // input only
    const EX0: u16 = 4; // shift register outputs: never both high
    const EX1: u16 = 5;
    const LED: u16 = 6; // held by a function
    const NETS: [usize; 7] = [0, 0, 1, 2, 3, 4, 5];

    mod server {
        use super::*;
        use tests_common::TestProcessEvents;
        use wire_weaver::BlockingEventOut;
        use ww_pin_diag::{Interned, PinDiagError};

        pub struct Server {
            pub points: Vec<Point<'static>>,
            pub states: Vec<Option<PointState>>,
        }

        impl Server {
            pub fn new() -> Self {
                let p = |name, group, caps| Point {
                    name: Interned::Text(name),
                    group: Interned::Text(group),
                    caps,
                };
                let points = vec![
                    p("P0", "mcu", ALL),
                    p("P1", "mcu", ALL),
                    p("P2", "mcu", ALL),
                    p("DET", "mcu", INPUT_ONLY),
                    // a firmware with a string table sends ids
                    Point {
                        name: Interned::Id(UNib32(17)),
                        group: Interned::Id(UNib32(18)),
                        caps: DRIVE_ONLY,
                    },
                    p("EX1", "chain", DRIVE_ONLY),
                    p("LED", "mcu", ALL),
                ];
                Server {
                    states: vec![None; points.len()],
                    points,
                }
            }

            fn high(&self, point: u16) -> bool {
                matches!(
                    self.states[point as usize],
                    Some(PointState::Output {
                        level: Level::High,
                        ..
                    })
                )
            }

            /// Level of a net: a driver wins (a normal one over a weak one, low over high between equals), then the
            /// external pull-up of P2's net, then the internal pulls; floating reads low.
            fn net_level(&self, net: usize) -> Level {
                let on_net = || (0..self.states.len()).filter(move |&i| NETS[i] == net);
                let mut best: Option<(bool, Level)> = None;
                for i in on_net() {
                    if let Some(PointState::Output { level, drive }) = self.states[i] {
                        let strong = drive == Drive::Normal;
                        best = match best {
                            Some((s, l)) if s && !strong => Some((s, l)),
                            Some((s, Level::Low)) if s == strong => Some((s, Level::Low)),
                            _ => Some((strong, level)),
                        };
                    }
                }
                if let Some((_, level)) = best {
                    return level;
                }
                if net == NETS[P2 as usize] {
                    return Level::High;
                }
                let pulled_up = on_net()
                    .any(|i| matches!(self.states[i], Some(PointState::Input { pull: Pull::Up })));
                if pulled_up { Level::High } else { Level::Low }
            }
        }

        impl api_impl::PinDiagTestConsts for Server {
            fn pins_points(&self) -> RefVec<'_, Point<'_>> {
                RefVec::Slice {
                    slice: &self.points,
                }
            }
        }

        impl Server {
            fn pins_set(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                point: u16,
                state: PointState,
            ) -> RpcResult<Result<(), PinDiagError<'_>>> {
                let Some(p) = self.points.get(point as usize) else {
                    return Ready(Err(PinDiagError::NoSuchPoint));
                };
                let capable = match state {
                    PointState::Input { pull: Pull::None } => p.caps.read,
                    PointState::Input { pull: Pull::Up } => p.caps.pull_up,
                    PointState::Input { pull: Pull::Down } => p.caps.pull_down,
                    PointState::Input { .. } => false,
                    PointState::Output {
                        level: Level::Low, ..
                    } => p.caps.drive_low,
                    PointState::Output {
                        level: Level::High, ..
                    } => p.caps.drive_high,
                    // a state from a newer version of the trait
                    PointState::Unknown(_) => false,
                };
                if !capable {
                    return Ready(Err(PinDiagError::NotCapable));
                }
                if point == LED {
                    return Ready(Err(PinDiagError::Common(CommonError::InUse {
                        by: MaxLen::new_unchecked("indication"),
                    })));
                }
                let other = match point {
                    EX0 => Some(EX1),
                    EX1 => Some(EX0),
                    _ => None,
                };
                let wants_high = matches!(
                    state,
                    PointState::Output {
                        level: Level::High,
                        ..
                    }
                );
                if wants_high && other.is_some_and(|o| self.high(o)) {
                    return Ready(Err(PinDiagError::Refused {
                        rule: Interned::Text("one source at a time"),
                    }));
                }
                self.states[point as usize] = Some(state);
                Ready(Ok(()))
            }

            fn pins_read(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                point: u16,
            ) -> RpcResult<Result<Level, PinDiagError<'_>>> {
                match self.points.get(point as usize) {
                    None => Ready(Err(PinDiagError::NoSuchPoint)),
                    Some(p) if !p.caps.read => Ready(Err(PinDiagError::NotCapable)),
                    Some(_) => Ready(Ok(self.net_level(NETS[point as usize]))),
                }
            }

            fn pins_release_all(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<()> {
                self.states.iter_mut().for_each(|s| *s = None);
                Ready(())
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                pin_diag_api :: PinDiagTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                const_model = "POINTS=fn",
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
                pin_diag_api :: PinDiagTest for super::StdClient,
                client = "std_client",
            );
        }
    }

    use std_client::StdClient;

    async fn start(path: &str) -> (TestDevice, StdClient) {
        let _ = tracing_subscriber::fmt::try_init();
        let device = tests_common::start_device(
            path,
            server::Server::new(),
            pin_diag_api::PIN_DIAG_TEST_FULL_GID,
            server::api_impl::api_hash(),
        );
        let client = StdClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");
        (device, client)
    }

    fn output(level: Level, drive: Drive) -> PointState {
        PointState::Output { level, drive }
    }

    fn input(pull: Pull) -> PointState {
        PointState::Input { pull }
    }

    fn device_error<T: std::fmt::Debug>(
        r: Result<T, wire_weaver_client::CallError<PinDiagErrorOwned>>,
    ) -> PinDiagErrorOwned {
        r.unwrap_err().device().cloned().expect("a device error")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn points_with_names_as_text_or_ids() {
        let (_device, client) = start("pin_diag/points").await;
        let points = client.pins().read_POINTS().read().await.unwrap();
        assert_eq!(points.len(), 7);
        assert_eq!(points[0].name, InternedOwned::Text("P0".into()));
        assert_eq!(points[0].caps, ALL);
        assert_eq!(points[DET as usize].caps, INPUT_ONLY);
        assert_eq!(points[EX0 as usize].name, InternedOwned::Id(UNib32(17)));
        assert_eq!(points[EX0 as usize].name.to_string(), "#17");
        assert_eq!(points[EX1 as usize].group.to_string(), "chain");
    }

    /// The tests a host runs: pull classification, drive and read back, shorts between points.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn set_and_read_find_pulls_and_shorts() {
        let (_device, client) = start("pin_diag/probe").await;
        let mut pins = client.pins();
        // P2 has an external pull-up: high even against the internal pull-down
        pins.set(P2, input(Pull::Down)).call().await.unwrap();
        assert_eq!(pins.read(P2).call().await.unwrap(), Level::High);
        // P0 floats: it follows the internal pull
        pins.set(P0, input(Pull::Up)).call().await.unwrap();
        assert_eq!(pins.read(P0).call().await.unwrap(), Level::High);
        pins.set(P0, input(Pull::Down)).call().await.unwrap();
        pins.set(P1, input(Pull::Down)).call().await.unwrap();
        assert_eq!(pins.read(P0).call().await.unwrap(), Level::Low);

        // P1 driven high: P0, on the same net, follows: a short
        pins.set(P1, output(Level::High, Drive::Normal))
            .call()
            .await
            .unwrap();
        assert_eq!(pins.read(P1).call().await.unwrap(), Level::High);
        assert_eq!(pins.read(P0).call().await.unwrap(), Level::High);
        // ... and P0 driving low weakly loses to it: read back while driving shows the short
        pins.set(P0, output(Level::Low, Drive::Weak))
            .call()
            .await
            .unwrap();
        assert_eq!(pins.read(P0).call().await.unwrap(), Level::High);

        // release: everything back, P0 floats low again
        pins.release_all().call().await.unwrap();
        assert_eq!(pins.read(P0).call().await.unwrap(), Level::Low);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn refusals() {
        let (_device, client) = start("pin_diag/refusals").await;
        let mut pins = client.pins();

        assert_eq!(
            device_error(pins.read(99).call().await),
            PinDiagErrorOwned::NoSuchPoint
        );
        // input only, and a shift register output that can't be read
        let r = pins
            .set(DET, output(Level::Low, Drive::Normal))
            .call()
            .await;
        assert_eq!(device_error(r), PinDiagErrorOwned::NotCapable);
        assert_eq!(
            device_error(pins.read(EX0).call().await),
            PinDiagErrorOwned::NotCapable
        );
        // held by a function
        let r = pins.set(LED, input(Pull::None)).call().await;
        assert!(matches!(
            device_error(r),
            PinDiagErrorOwned::Common(CommonErrorOwned::InUse { .. })
        ));
        // the board's rule: EX0 and EX1 never both high; off first, then the other one is fine
        pins.set(EX0, output(Level::High, Drive::Normal))
            .call()
            .await
            .unwrap();
        let r = pins
            .set(EX1, output(Level::High, Drive::Normal))
            .call()
            .await;
        let err = device_error(r);
        assert_eq!(
            err,
            PinDiagErrorOwned::Refused {
                rule: InternedOwned::Text("one source at a time".into())
            }
        );
        assert_eq!(
            err.to_string(),
            "The board's rules forbid this now: one source at a time"
        );
        pins.set(EX0, output(Level::Low, Drive::Normal))
            .call()
            .await
            .unwrap();
        pins.set(EX1, output(Level::High, Drive::Normal))
            .call()
            .await
            .unwrap();
    }

    /// What `ww pins test` does: PinDiag found at `pins` by introspection, every test through the dynamic client.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn probe_through_introspection() {
        use wire_weaver_client::{ClientConfig, DynClient};
        use ww_pin_probe::device::{DevicePins, Names};
        use ww_pin_probe::plan::PullClass;
        use ww_pin_probe::probe::{Check, Setup, run};
        use ww_pin_probe::select::select;

        let _ = tracing_subscriber::fmt::try_init();
        let device = tests_common::start_device(
            "pin_diag/probe_dyn",
            server::Server::new(),
            pin_diag_api::PIN_DIAG_TEST_FULL_GID,
            server::api_impl::api_hash(),
        );
        let client = DynClient::from_config(ClientConfig::new().in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");
        assert!(DevicePins::find(&client, Some("diag")).is_err());
        let mut pins = DevicePins::find(&client, None).unwrap();
        assert_eq!(pins.at.name, "pins");
        let raw = pins.raw_points().await.unwrap();
        // EX0's name is an id: shown as #17, the device has no build id to find an ELF by
        let names = Names::for_device(&client, &raw).await;
        assert!(names.note.as_deref().unwrap().contains("no build id"));
        let points = names.points(&raw);
        assert_eq!(points[EX0 as usize].name, "#17");

        let selected = select(&points, &["P0..=P2,DET,#17,EX1,LED"]).unwrap();
        let setup = Setup::new(&points, &selected, &[], None).unwrap();
        let report = run(&mut pins, &setup, std::future::pending::<()>(), |_, _| {})
            .await
            .unwrap();
        let point = |id: u16| report.point(id).unwrap();
        assert_eq!(point(P2).pulls, Check::Done(PullClass::PullUp));
        assert_eq!(point(P0).gnd, Check::Done(false));
        assert_eq!(point(DET).pulls, Check::Done(PullClass::Floating));
        assert!(matches!(point(DET).gnd, Check::NotChecked(_)));
        assert!(matches!(point(LED).pulls, Check::Refused(_)));
        assert_eq!(point(EX0).pulls, Check::NotChecked("can't be read".into()));
        let pairs: Vec<_> = report.connections.iter().map(|c| (c.a, c.b)).collect();
        assert_eq!(pairs, vec![(P0, P1)]);
        assert_eq!(report.shorts(), 1);
        // everything given back: P0 floats low again
        let mut check = DevicePins::find(&client, Some("pins")).unwrap();
        use ww_pin_probe::Pins;
        assert_eq!(check.read(P0).await.unwrap(), Level::Low);
    }
}

//! `ww_si` quantities through the dynamic client (CLT-31): values read from a device over the in-process transport,
//! recognized from introspection data alone and shown with their units, the same text as the typed `Display`. `Fixed`
//! values (STD-55) written with and without their default bounds, carried with all four arguments in the model.

#[cfg(test)]
mod tests {
    use quantities_api::{BoardTemp, DegreePerSecond, Mdps70, Rpm};
    use std::sync::{Arc, Mutex};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver_client::quantity::{
        CountKind, DynCount, DynPrefix, DynQuantity, DynUnit, is_quantity, unit_of,
    };
    use wire_weaver_client::ww_self::{
        self, ApiItemKindOwned, FieldsValueOwned, GenericArgOwned, TypeOwned, ValueOwned,
    };
    use wire_weaver_client::{ClientConfig, DynClient, DynResource};
    use ww_si::origins::Angle;
    use ww_si::prefix::{Centi, FullScale, Kibi, Micro, Milli, One, Rational};
    use ww_si::units::{
        self, AngularVelocity, Degree, Frame, Item, Meter, Per, Ratio, Sample, Times, Torque,
    };
    use ww_si::ww_numeric::NumericValue;
    use ww_si::{
        Ampere, AnyQuantity, Dim, Fixed, FixedScale, Index, IndexFrom1, Magnitude, Micros, Point,
        Prefix, Quantity, Range, RangeDec, SIExpr, Scale, Second, Seq, Timed, Unbounded, Unit,
        Volt,
    };

    type Damping = Quantity<Per<Torque, AngularVelocity>>;
    type Rail = Quantity<units::Volt, RangeDec<0, 33, -1>>;
    /// `B` left out, as in the API's `timeout`
    type Timeout = Fixed<units::Second, Milli, u16>;
    type Window = Fixed<units::Second, Milli, u16, Range<1, 5000>>;
    type Size = Fixed<units::Byte, One, u32>;
    type Adc = Fixed<units::Volt, FullScale<Rational<33, 10>, 12>, u16>;
    type Gyro = Fixed<DegreePerSecond, Mdps70, i16>;
    type Belt = Fixed<Times<Per<Ratio, units::Second>, Meter>, Milli, u16>;
    type BoardTemperature = Point<BoardTemp, Fixed<units::Kelvin, Rational<1, 256>, i16>>;
    type Heading = Point<Angle<Degree>, Fixed<Degree, Centi, u16>>;
    type Uptime = Fixed<units::Second, Micro, u64>;
    type Buffer = Fixed<units::Byte, Kibi, u16>;
    type Delay = Fixed<units::Second, Micro, u32, Range<0, 5000000>>;
    type Waited = Fixed<units::Second, Milli, u32>;
    type Input = IndexFrom1<Item, u8, Range<1, 4>>;

    struct Values {
        voltage: Volt,
        rail: Rail,
        duty: Quantity<units::Ratio>,
        damping: Damping,
        speed: Quantity<Rpm>,
        /// The `AnyQuantity` property, serialized.
        sensor: Vec<u8>,
        timeout: Timeout,
        window: Window,
        uptime: Uptime,
        buffer: Buffer,
        /// The last `delay` the `wait` method got.
        delay: Option<Delay>,
        input: Input,
        sample: Index<Sample, u32>,
        frame: Seq<Frame, u16>,
    }

    /// A pressure sensor input in bar, a unit `ww_si` doesn't have: sent with its dimension and scale.
    fn bar() -> SIExpr<'static> {
        SIExpr::Named {
            name: "Bar",
            symbol: "bar",
            dim: Dim::new(-2, -1, 1, 0, 0, 0, 0),
            scale: Scale::Pow10(5),
        }
    }

    fn sensor() -> AnyQuantity<'static> {
        AnyQuantity::new(Prefix::Milli, 1013u16, bar())
    }

    fn values() -> Values {
        Values {
            voltage: Volt::milli(3300u16),
            rail: Rail::try_new(Prefix::Milli, 1800u16).unwrap(),
            duty: Quantity::new(
                Prefix::from_magnitude(&Magnitude::ratio(1, 65535)).unwrap(),
                32768u16,
            ),
            damping: Damping::from_f32(1.2e-4),
            speed: Quantity::<Rpm>::base(1500u16),
            sensor: sensor().to_ww_bytes(&mut [0u8; 64]).unwrap().to_vec(),
            timeout: Fixed::new(250),
            window: Window::bounded(100),
            uptime: Uptime::new(1_730_000_000_000_000),
            buffer: Buffer::new(4),
            delay: None,
            input: Input::try_new(2).unwrap(),
            sample: Index::new(1000),
            frame: Seq::new(41),
        }
    }

    fn history() -> Vec<Timed<Ampere>> {
        (0..20u32)
            .map(|k| Timed {
                t: Second::from_parts(Prefix::Micro, NumericValue::U32(1_000_000 + k * 40)),
                value: Ampere::from_parts(Prefix::Milli, NumericValue::I32(152 - k as i32)),
            })
            .collect()
    }

    fn sizes() -> Vec<Timed<Size>> {
        (0..4u32)
            .map(|k| Timed {
                t: Second::from_parts(Prefix::Micro, NumericValue::U32(100 + k)),
                value: Fixed::new(512 << k),
            })
            .collect()
    }

    fn ticks() -> Vec<Timed<Micros<u32>>> {
        (0..4u32)
            .map(|k| Timed {
                t: Second::from_parts(Prefix::Micro, NumericValue::U32(k)),
                value: Fixed::new(1000 * k),
            })
            .collect()
    }

    mod server {
        use super::*;
        use tests_common::TestProcessEvents;
        use wire_weaver::shrink_wrap::Series;
        use wire_weaver::{GetResult, SetResult};

        pub struct Server {
            pub values: Arc<Mutex<Values>>,
            pub history: Vec<Timed<Ampere>>,
            pub sizes: Vec<Timed<Size>>,
            pub ticks: Vec<Timed<Micros<u32>>>,
            /// Bytes of the sensor value `get_sensor` returns.
            pub sensor: Vec<u8>,
        }

        impl Server {
            fn set_voltage(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Volt,
            ) -> SetResult<()> {
                self.values.lock().unwrap().voltage = value;
                SetResult::Set
            }

            fn get_voltage(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Volt, ()> {
                GetResult::Value(self.values.lock().unwrap().voltage)
            }

            fn set_rail(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Rail,
            ) -> SetResult<()> {
                self.values.lock().unwrap().rail = value;
                SetResult::Set
            }

            fn get_rail(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Rail, ()> {
                GetResult::Value(self.values.lock().unwrap().rail)
            }

            fn get_duty(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Quantity<units::Ratio>, ()> {
                GetResult::Value(self.values.lock().unwrap().duty)
            }

            fn history(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Series<'_, Timed<Ampere>>> {
                Ready(Series::new(&self.history))
            }

            fn set_damping(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Damping,
            ) -> SetResult<()> {
                self.values.lock().unwrap().damping = value;
                SetResult::Set
            }

            fn get_damping(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Damping, ()> {
                GetResult::Value(self.values.lock().unwrap().damping)
            }

            fn set_speed(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Quantity<Rpm>,
            ) -> SetResult<()> {
                self.values.lock().unwrap().speed = value;
                SetResult::Set
            }

            fn get_speed(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Quantity<Rpm>, ()> {
                GetResult::Value(self.values.lock().unwrap().speed)
            }

            fn set_sensor(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: AnyQuantity<'_>,
            ) -> SetResult<()> {
                let mut buf = [0u8; 256];
                let bytes = value.to_ww_bytes(&mut buf).unwrap();
                self.values.lock().unwrap().sensor = bytes.to_vec();
                SetResult::Set
            }

            fn get_sensor(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<AnyQuantity<'_>, ()> {
                self.sensor = self.values.lock().unwrap().sensor.clone();
                match AnyQuantity::from_ww_bytes(&self.sensor) {
                    Ok(q) => GetResult::Value(q),
                    Err(_) => GetResult::GetError(()),
                }
            }
        }

        impl Server {
            fn set_timeout(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Timeout,
            ) -> SetResult<()> {
                self.values.lock().unwrap().timeout = value;
                SetResult::Set
            }

            fn get_timeout(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Timeout, ()> {
                GetResult::Value(self.values.lock().unwrap().timeout)
            }

            // `B` written out: the same type as `Timeout`
            fn set_timeout_full(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Fixed<units::Second, Milli, u16, Unbounded>,
            ) -> SetResult<()> {
                self.values.lock().unwrap().timeout = value;
                SetResult::Set
            }

            fn get_timeout_full(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Fixed<units::Second, Milli, u16, Unbounded>, ()> {
                GetResult::Value(self.values.lock().unwrap().timeout)
            }

            fn get_adc(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Adc, ()> {
                GetResult::Value(Adc::new(2048))
            }

            fn get_gyro(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Gyro, ()> {
                GetResult::Value(Gyro::new(-100))
            }

            fn get_belt(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Belt, ()> {
                GetResult::Value(Belt::new(1500))
            }

            fn get_board_temp(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<BoardTemperature, ()> {
                GetResult::Value(BoardTemperature::from_raw(128))
            }

            fn get_heading(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Heading, ()> {
                GetResult::Value(Heading::from_raw(9000))
            }

            fn set_window(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Window,
            ) -> SetResult<()> {
                self.values.lock().unwrap().window = value;
                SetResult::Set
            }

            fn get_window(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Window, ()> {
                GetResult::Value(self.values.lock().unwrap().window)
            }

            fn sizes(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Series<'_, Timed<Size>>> {
                Ready(Series::new(&self.sizes))
            }

            fn ticks(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Series<'_, Timed<Micros<u32>>>> {
                Ready(Series::new(&self.ticks))
            }
        }

        /// CLT-54: values the clients convert into and out of their scale and storage.
        impl Server {
            fn set_uptime(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Uptime,
            ) -> SetResult<()> {
                self.values.lock().unwrap().uptime = value;
                SetResult::Set
            }

            fn get_uptime(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Uptime, ()> {
                GetResult::Value(self.values.lock().unwrap().uptime)
            }

            fn set_buffer(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Buffer,
            ) -> SetResult<()> {
                self.values.lock().unwrap().buffer = value;
                SetResult::Set
            }

            fn get_buffer(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Buffer, ()> {
                GetResult::Value(self.values.lock().unwrap().buffer)
            }

            fn wait(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                delay: Delay,
            ) -> RpcResult<Waited> {
                self.values.lock().unwrap().delay = Some(delay);
                Ready(Waited::new(delay.get() / 1000))
            }

            fn samples(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
                Ready(())
            }

            fn close_samples(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn sideband_samples(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                Some(sideband)
            }

            fn set_input(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Input,
            ) -> SetResult<()> {
                self.values.lock().unwrap().input = value;
                SetResult::Set
            }

            fn get_input(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Input, ()> {
                GetResult::Value(self.values.lock().unwrap().input)
            }

            fn set_sample(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Index<Sample, u32>,
            ) -> SetResult<()> {
                self.values.lock().unwrap().sample = value;
                SetResult::Set
            }

            fn get_sample(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Index<Sample, u32>, ()> {
                GetResult::Value(self.values.lock().unwrap().sample)
            }

            fn set_frame(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Seq<Frame, u16>,
            ) -> SetResult<()> {
                self.values.lock().unwrap().frame = value;
                SetResult::Set
            }

            fn get_frame(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Seq<Frame, u16>, ()> {
                GetResult::Value(self.values.lock().unwrap().frame)
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                quantities_api :: Quantities for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
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

    fn start(path: &str) -> (TestDevice, Arc<Mutex<Values>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let values = Arc::new(Mutex::new(values()));
        let server = server::Server {
            values: values.clone(),
            history: history(),
            sizes: sizes(),
            ticks: ticks(),
            sensor: vec![],
        };
        let device = tests_common::start_device(
            path,
            server,
            quantities_api::QUANTITIES_FULL_GID,
            server::api_impl::api_hash(),
        );
        (device, values)
    }

    async fn connect(device: &TestDevice) -> (DynClient, DynResource) {
        let client = DynClient::from_config(ClientConfig::new().in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");
        let bundle = client.device_introspect().unwrap().api_bundle.clone();
        (client, DynResource::root(bundle))
    }

    /// Reads property `name` and its type.
    async fn read(client: &DynClient, root: &DynResource, name: &str) -> (ValueOwned, TypeOwned) {
        let property = root.child(name).unwrap();
        let value = property.read(client.cmd()).await.unwrap();
        let ApiItemKindOwned::Property { ty, .. } = &property.item().unwrap().kind else {
            panic!("property");
        };
        (value, ty.clone())
    }

    async fn read_quantity(client: &DynClient, root: &DynResource, name: &str) -> DynQuantity {
        let (value, ty) = read(client, root, name).await;
        DynQuantity::from_value(&value, &ty, root.bundle()).expect("a quantity")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn named_unit_property() {
        let (device, _values) = start("quantities/volt");
        let (client, root) = connect(&device).await;
        let v = read_quantity(&client, &root, "voltage").await;
        assert_eq!(v.unit, DynUnit::Known(units::Volt::EXPR));
        assert_eq!(
            (v.prefix.clone(), v.value),
            (Prefix::Milli.into(), NumericValue::U16(3300))
        );
        assert_eq!(v.to_string(), "3300 mV");
        assert_eq!(format!("{v:#}"), "3.3 V");
        assert_eq!(v.to_string(), values().voltage.to_string());
        assert_eq!(format!("{v:#}"), format!("{:#}", values().voltage));
        assert_eq!(v.to_si_f64(), Some(3.3));
        assert_eq!(v.unit.dim().unwrap().to_string(), "kg·m²·s⁻³·A⁻¹");
        assert_eq!(v.unit.name(), "Volt");
    }

    /// An `Exact` prefix (STD-59) comes through the dynamic client with its scale, shown as the typed quantity shows it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn exact_prefix_property() {
        let (device, _values) = start("quantities/exact");
        let (client, root) = connect(&device).await;
        let d = read_quantity(&client, &root, "duty").await;
        assert_eq!(d.unit, DynUnit::Known(units::Ratio::EXPR));
        assert_eq!(
            (d.prefix.clone(), d.value),
            (values().duty.prefix.into(), NumericValue::U16(32768))
        );
        assert_eq!(d.to_string(), values().duty.to_string());
        assert_eq!(d.to_string(), "32768·1/65535");
        assert_eq!(d.to_f64(), 32768.0 / 65535.0);
    }

    /// A scale from a newer device version (DER-25) comes through as `Unknown(tag)`: shown with its tag, not converted.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unknown_prefix_property() {
        let (device, values) = start("quantities/unknown");
        values.lock().unwrap().duty = Quantity::new(Prefix::Unknown(1000), 5u16);
        let (client, root) = connect(&device).await;
        let d = read_quantity(&client, &root, "duty").await;
        assert_eq!(
            (d.prefix.clone(), d.value),
            (Prefix::Unknown(1000).into(), NumericValue::U16(5))
        );
        assert_eq!(d.to_string(), "5·<scale 1000>");
        assert!(d.to_f64().is_nan());
        assert_eq!(d.to_si_f64(), None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn compound_unit_property() {
        let (device, _values) = start("quantities/per");
        let (client, root) = connect(&device).await;
        let b = read_quantity(&client, &root, "damping").await;
        assert_eq!(
            b.unit,
            DynUnit::Per(
                Box::new(DynUnit::Known(Torque::EXPR)),
                Box::new(DynUnit::Known(AngularVelocity::EXPR))
            )
        );
        assert_eq!(b.unit.symbol(), "N·m/(rad/s)");
        assert_eq!(b.unit.name(), "Per<Torque, AngularVelocity>");
        assert_eq!(b.to_string(), values().damping.to_string());
        assert_eq!(b.to_string(), "0.00012 N·m/(rad/s)");
        assert_eq!(b.unit.dim().unwrap().to_string(), "kg·m²·s⁻¹·rad⁻²");
        assert_eq!(b.to_si_f64(), Some(values().damping.to_si_f64()));
    }

    /// A unit of the API's own crate, declared with `unit!`: the bundle carries its definition (INT-13), so the client
    /// shows and converts it as the typed quantity does, without a table of its own.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn own_unit_from_its_definition() {
        let (device, values) = start("quantities/rpm");
        let (client, root) = connect(&device).await;
        let speed = read_quantity(&client, &root, "speed").await;
        let typed = values.lock().unwrap().speed;
        assert_eq!(
            speed.unit,
            DynUnit::Named {
                name: "Rpm".into(),
                symbol: "rpm".into(),
                dim: Rpm::DIM.into(),
                scale: Scale::Exact(Magnitude::new(1, 30, 0, 0, 1)),
            }
        );
        assert_eq!(speed.to_string(), "1500 rpm");
        assert_eq!(speed.to_string(), typed.to_string());
        assert_eq!(speed.unit.dim().unwrap().to_string(), "s⁻¹·rad");
        assert_eq!(speed.to_si_f64(), Some(typed.to_si_f64()));
        assert!((speed.to_si_f64().unwrap() - 50.0 * std::f64::consts::PI).abs() < 1e-9);
    }

    /// Units resolved through the bundle's definitions to a canonical expression: the same named units to the same
    /// powers are the same unit however they are written, units of one dimension convert by an exact factor.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn canonical_units_of_the_model() {
        let (device, _values) = start("quantities/canonical");
        let (client, root) = connect(&device).await;
        let bundle = root.bundle();
        let unit = |ty: &TypeOwned| {
            let Ok(TypeOwned::Struct(q)) = ty.get_in_line(bundle) else {
                panic!("Quantity<U, B>");
            };
            let [GenericArgOwned::Type(u), _] = q.instance_of.as_ref().unwrap().args.as_slice()
            else {
                panic!("Quantity<U, B>");
            };
            ww_self::units::unit_of(u, bundle).unwrap()
        };
        let (_, speed) = read(&client, &root, "speed").await;
        let (_, damping) = read(&client, &root, "damping").await;
        let (_, voltage) = read(&client, &root, "voltage").await;
        let (rpm, damping, volt) = (unit(&speed), unit(&damping), unit(&voltage));
        assert_eq!(rpm.to_string(), "rpm");
        assert_eq!(
            rpm.powers[0].marker,
            ww_self::units::MarkerName {
                crate_id: "quantities_api".into(),
                name: "Rpm".into()
            }
        );
        assert_eq!(rpm.magnitude, Magnitude::new(1, 30, 0, 0, 1));
        // N·m / (rad/s), two named units: sorted by marker name, a compound symbol to a power in parentheses
        assert_eq!(damping.to_string(), "(rad/s)⁻¹·N·m");
        assert!(
            damping
                .dim
                .as_dim()
                .same(Per::<Torque, AngularVelocity>::DIM)
        );
        assert_eq!(volt.to_string(), "V");
        // a different unit of another dimension: no factor between them
        assert_ne!(rpm, volt);
        assert_eq!(rpm.factor_to(&volt), None);
        assert_eq!(rpm.factor_to(&rpm), Some(Magnitude::ONE));
        // scales: a prefix, a full-scale step, a scale marker of the API's crate
        let mut scales = Vec::new();
        for name in ["timeout", "adc", "gyro"] {
            let (_, ty) = read(&client, &root, name).await;
            let Ok(TypeOwned::Struct(f)) = ty.get_in_line(bundle) else {
                panic!("Fixed");
            };
            let [GenericArgOwned::Type(u), GenericArgOwned::Type(s), ..] =
                f.instance_of.as_ref().unwrap().args.as_slice()
            else {
                panic!("Fixed<U, S, ..>");
            };
            scales.push((
                ww_self::units::unit_of(u, bundle).unwrap(),
                ww_self::units::scale_of(s, bundle).unwrap(),
            ));
        }
        let [(_, timeout), (_, adc), (gyro_unit, gyro_scale)]: [_; 3] = scales.try_into().unwrap();
        assert_eq!(timeout, Magnitude::pow10(-3));
        assert_eq!(adc, <FullScale<Rational<33, 10>, 12> as FixedScale>::SCALE);
        assert_eq!(adc, Magnitude::ratio(33, 40960));
        assert_eq!(gyro_scale, Magnitude::ratio(7, 100));
        assert_eq!(gyro_scale, <Mdps70 as FixedScale>::SCALE);
        assert_eq!(gyro_unit.to_string(), "°·s⁻¹");
        assert_eq!(gyro_unit.magnitude, Magnitude::new(1, 180, 0, 0, 1));
        let one = ww_self::units::CanonicalUnit::one();
        assert_eq!(one.to_string(), "1");
        // a dimensionless ratio converts into rpm by nothing: the dimensions differ
        assert_eq!(one.factor_to(&rpm), None);
        // Ratio is no named unit of the product: (1/s)·m is m·s⁻¹
        let (_, belt) = read(&client, &root, "belt").await;
        let Ok(TypeOwned::Struct(f)) = belt.get_in_line(bundle) else {
            panic!("Fixed");
        };
        let Some(GenericArgOwned::Type(u)) = f.instance_of.as_ref().unwrap().args.first() else {
            panic!("Fixed<U, ..>");
        };
        let belt = ww_self::units::unit_of(u, bundle).unwrap();
        assert_eq!(belt.to_string(), "m·s⁻¹");
        let names: Vec<_> = belt.powers.iter().map(|p| p.marker.name.as_str()).collect();
        assert_eq!(names, ["units::Meter", "units::Second"]);
    }

    /// INT-13: the origins of points come with their definitions, an origin of the API's crate declared above °C
    /// written out whole, and an angle with its turn in its unit.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn origins_of_the_model() {
        let (device, _values) = start("quantities/origins");
        let (client, root) = connect(&device).await;
        let bundle = root.bundle();
        let mut origins = Vec::new();
        for name in ["board_temp", "heading"] {
            let (_, ty) = read(&client, &root, name).await;
            let Ok(TypeOwned::Struct(p)) = ty.get_in_line(bundle) else {
                panic!("Point");
            };
            let [GenericArgOwned::Type(o), ..] = p.instance_of.as_ref().unwrap().args.as_slice()
            else {
                panic!("Point<O, F>");
            };
            origins.push(ww_self::units::origin_of(o, bundle).unwrap());
        }
        let [board, heading]: [_; 2] = origins.try_into().unwrap();
        let compiled = <BoardTemp as ww_si::Origin>::DEF;
        assert_eq!(board.symbol, "°C");
        assert_eq!(board.unit.symbol, "K");
        assert_eq!(board.offset, compiled.offset);
        assert_eq!(board.offset, ww_si::Offset::decimal(29815, -2));
        // shown in °C, from 273.15 K
        assert_eq!(board.shown, ww_si::Offset::decimal(27315, -2));
        assert_eq!((board.modulus, board.reference), (None, None));
        let compiled = <Angle<Degree> as ww_si::Origin>::DEF;
        assert_eq!(heading.modulus, compiled.modulus);
        assert_eq!(heading.modulus, Some(Magnitude::ratio(360, 1)));
        assert_eq!(
            (heading.symbol.as_str(), heading.unit.symbol.as_str()),
            ("°", "°")
        );
        assert_eq!(heading.unit.magnitude, Magnitude::new(1, 180, 0, 0, 1));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn timed_series_items() {
        let (device, _values) = start("quantities/series");
        let (client, root) = connect(&device).await;
        let method = root.child("history").unwrap();
        let ApiItemKindOwned::Method {
            return_ty: Some(TypeOwned::Series(item_ty)),
            ..
        } = &method.item().unwrap().kind
        else {
            panic!("a method returning a Series");
        };
        let bundle = root.bundle();
        let Ok(TypeOwned::Struct(timed)) = item_ty.get_in_line(bundle) else {
            panic!("Timed<Ampere>");
        };
        assert!(
            unit_of(item_ty, bundle).is_none(),
            "Timed is not a quantity"
        );
        let wire_weaver_client::ww_self::FieldsOwned::Named(timed_fields) = &timed.fields else {
            panic!("named fields");
        };
        let field_ty = |name: &str| {
            timed_fields
                .iter()
                .find(|f| f.ident.as_deref() == Some(name))
                .unwrap()
                .ty
                .clone()
        };
        let Some(ValueOwned::Vec(items)) = method.call(client.cmd(), &[]).await.unwrap() else {
            panic!("items");
        };
        assert_eq!(items.len(), history().len());
        for (item, expected) in items.iter().zip(history()) {
            let ValueOwned::Struct {
                fields: wire_weaver_client::ww_self::FieldsValueOwned::Named(fields),
            } = item
            else {
                panic!("Timed");
            };
            let t = DynQuantity::from_value(&fields[0].1, &field_ty("t"), bundle).unwrap();
            let i = DynQuantity::from_value(&fields[1].1, &field_ty("value"), bundle).unwrap();
            assert_eq!(t.to_string(), expected.t.to_string());
            assert_eq!(i.to_string(), expected.value.to_string());
            assert_eq!(i.unit, DynUnit::Known(units::Ampere::EXPR));
        }
        let ValueOwned::Struct {
            fields: wire_weaver_client::ww_self::FieldsValueOwned::Named(first),
        } = &items[0]
        else {
            unreachable!()
        };
        let i = DynQuantity::from_value(&first[1].1, &field_ty("value"), bundle).unwrap();
        assert_eq!(i.to_string(), "152 mA");
        assert_eq!(format!("{i:#}"), "0.152 A");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn quantities_written_back() {
        let (device, values) = start("quantities/write");
        let (client, root) = connect(&device).await;
        let mut v = read_quantity(&client, &root, "voltage").await;
        v.prefix = Prefix::Micro.into();
        v.value = NumericValue::I32(-1500);
        let voltage = root.child("voltage").unwrap();
        let (_, ty) = read(&client, &root, "voltage").await;
        let value = v.to_value(&ty, root.bundle()).unwrap();
        voltage.write(client.cmd(), &value).await.unwrap();
        let written = values.lock().unwrap().voltage;
        assert!(written.same_repr(&Volt::micro(-1500i32)));
        assert_eq!(read_quantity(&client, &root, "voltage").await, v);

        // only into a quantity of the same unit
        let (_, damping) = read(&client, &root, "damping").await;
        assert_eq!(
            v.to_value(&damping, root.bundle()).unwrap_err(),
            "a quantity in Volt where one in Per<Torque, AngularVelocity> is expected"
        );
    }

    /// `q!(..)` and plain numbers (TOOL-9), written to the device and read back.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn quantities_as_rust_expressions() {
        use wire_weaver_client::expr::parse_value;
        let (device, values) = start("quantities/expr");
        let (client, root) = connect(&device).await;
        let write = async |name: &str, text: &str| {
            let (_, ty) = read(&client, &root, name).await;
            let value = parse_value(text, &ty, root.bundle())?;
            root.child(name)
                .unwrap()
                .write(client.cmd(), &value)
                .await
                .unwrap();
            anyhow::Ok(read_quantity(&client, &root, name).await)
        };

        write("voltage", "q!(-1500 uV)").await.unwrap();
        assert!(
            values
                .lock()
                .unwrap()
                .voltage
                .same_repr(&Volt::micro(-1500i32))
        );
        let v = write("voltage", "q!(3.3 kV)").await.unwrap();
        assert_eq!(
            (v.prefix, v.value),
            (Prefix::Kilo.into(), NumericValue::F32(3.3))
        );
        let v = write("voltage", "5").await.unwrap();
        assert_eq!(
            (v.prefix, v.value),
            (Prefix::Unit.into(), NumericValue::I32(5))
        );
        let b = write("damping", "q!(1.2e-4 N*m/(rad/s))").await.unwrap();
        assert_eq!(b.to_string(), "0.00012 N·m/(rad/s)");
        let s = write("sensor", r#"q!("20 mA")"#).await.unwrap();
        assert_eq!(
            (s.prefix, s.unit.symbol().as_str()),
            (Prefix::Milli.into(), "A")
        );

        assert_eq!(
            write("voltage", "q!(3 A)").await.unwrap_err().to_string(),
            "`3 A`: expected a quantity in Volt (V), e.g. `1 V` or with a prefix"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn any_quantity_property() {
        let (device, _values) = start("quantities/any");
        let (client, root) = connect(&device).await;
        let (_, ty) = read(&client, &root, "sensor").await;
        assert!(is_quantity(&ty, root.bundle()));
        assert!(
            unit_of(&ty, root.bundle()).is_none(),
            "the unit is in the value, not the type"
        );
        let p = read_quantity(&client, &root, "sensor").await;
        assert_eq!(
            p.unit,
            DynUnit::Named {
                name: "Bar".into(),
                symbol: "bar".into(),
                dim: Dim::new(-2, -1, 1, 0, 0, 0, 0).into(),
                scale: Scale::Pow10(5),
            }
        );
        // the device's own unit, shown and converted as the device's AnyQuantity shows it
        assert_eq!(p.to_string(), "1013 mbar");
        assert_eq!(p.to_string(), sensor().to_string());
        assert_eq!(format!("{p:#}"), format!("{:#}", sensor()));
        assert_eq!(p.to_si_f64(), sensor().to_si_f64());
        assert_eq!(p.unit.dim().unwrap().to_string(), "kg·m⁻¹·s⁻²");
        assert_eq!(p.unit.name(), "Bar");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn any_quantity_like_typed() {
        let (device, values) = start("quantities/any_typed");
        let (client, root) = connect(&device).await;
        let sensor = root.child("sensor").unwrap();
        let (_, sensor_ty) = read(&client, &root, "sensor").await;
        let bundle = root.bundle();

        // a typed quantity read from one property, written into the AnyQuantity: its unit goes along
        for name in ["voltage", "damping"] {
            let typed = read_quantity(&client, &root, name).await;
            let value = typed.to_value(&sensor_ty, bundle).unwrap();
            sensor.write(client.cmd(), &value).await.unwrap();
            let any = read_quantity(&client, &root, "sensor").await;
            // the same DynUnit as the typed property's, so shown the same
            assert_eq!(any, typed);
            assert_eq!(any.to_string(), typed.to_string());
            assert_eq!(format!("{any:#}"), format!("{typed:#}"));
            assert_eq!(any.to_si_f64(), typed.to_si_f64());
        }
        // the device got the unit tree of the typed quantity
        let bytes = values.lock().unwrap().sensor.clone();
        let got = AnyQuantity::from_ww_bytes(&bytes).unwrap();
        assert_eq!(
            got.to_quantity::<Per<Torque, AngularVelocity>>(),
            Some(Damping::from_f32(1.2e-4))
        );
        assert_eq!(got.to_string(), "0.00012 N·m/(rad/s)");

        // the API's own unit goes along with the dimension and factor its definition gives (INT-13)
        let speed = read_quantity(&client, &root, "speed").await;
        sensor
            .write(client.cmd(), &speed.to_value(&sensor_ty, bundle).unwrap())
            .await
            .unwrap();
        let any = read_quantity(&client, &root, "sensor").await;
        assert_eq!(any, speed);
        let bytes = values.lock().unwrap().sensor.clone();
        let got = AnyQuantity::from_ww_bytes(&bytes).unwrap();
        assert_eq!(
            got.to_quantity::<Rpm>(),
            Some(Quantity::<Rpm>::base(1500u16))
        );

        // a marker without a definition has nothing to send
        let unknown = DynQuantity {
            unit: DynUnit::Unknown {
                name: "Furlong".into(),
                docs: vec![],
            },
            ..speed
        };
        assert_eq!(
            unknown.to_value(&sensor_ty, bundle).unwrap_err(),
            "a quantity in Furlong has no dimension and scale to send in an AnyQuantity"
        );
    }

    /// The `Fixed` of a property or of the `Timed` items of a method's `Series`, with its name as `ww` shows it.
    fn fixed_name(root: &DynResource, name: &str) -> String {
        let item = root.child(name).unwrap();
        let ty = match &item.item().unwrap().kind {
            ApiItemKindOwned::Property { ty, .. } => ty.clone(),
            ApiItemKindOwned::Method {
                return_ty: Some(TypeOwned::Series(item_ty)),
                ..
            } => *item_ty.clone(),
            kind => panic!("{name}: {kind:?}"),
        };
        ty.human_name(false, root.bundle()).unwrap()
    }

    /// D13: `B = Unbounded` is Rust typing sugar; the walker fills it in, so the model has all four arguments
    /// whether the source wrote three or four, also inside `Timed`, a `Series` and an alias.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fixed_default_bounds_filled_in() {
        let (device, _values) = start("quantities/fixed_model");
        let (_client, root) = connect(&device).await;
        let ty = |name: &str| {
            let property = root.child(name).unwrap();
            let ApiItemKindOwned::Property { ty, .. } = &property.item().unwrap().kind else {
                panic!("property");
            };
            ty.clone()
        };
        assert_eq!(ty("timeout"), ty("timeout_full"));
        let timeout = ty("timeout");
        let Ok(TypeOwned::Struct(fixed)) = timeout.get_in_line(root.bundle()) else {
            panic!("Fixed");
        };
        let args = &fixed
            .instance_of
            .as_ref()
            .expect("an instance of Fixed")
            .args;
        assert_eq!(args.len(), 4);
        assert_eq!(
            fixed_name(&root, "timeout"),
            "Fixed<Second, Milli, u16, Unbounded>"
        );
        assert_eq!(
            fixed_name(&root, "window"),
            "Fixed<Second, Milli, u16, Range<1, 5000>>"
        );
        assert_eq!(
            fixed_name(&root, "sizes"),
            "Timed<Fixed<Byte, One, u32, Unbounded>>"
        );
        assert_eq!(
            fixed_name(&root, "ticks"),
            "Timed<Fixed<Second, Micro, u32, Unbounded>>"
        );
    }

    /// A `Quantity`'s bounds are its second argument in the model, `Unbounded` filled in where the source left it out
    /// (D13), and the device refuses a value outside them whatever its prefix (STD-59).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn quantity_bounds_in_the_model() {
        let (device, values) = start("quantities/quantity_bounds");
        let (client, root) = connect(&device).await;
        assert_eq!(fixed_name(&root, "voltage"), "Quantity<Volt, Unbounded>");
        assert_eq!(
            fixed_name(&root, "rail"),
            "Quantity<Volt, RangeDec<0, 33, -1>>"
        );
        assert_eq!(
            fixed_name(&root, "history"),
            "Timed<Quantity<Ampere, Unbounded>>"
        );

        let rail = root.child("rail").unwrap();
        let (_, ty) = read(&client, &root, "rail").await;
        let mut v = read_quantity(&client, &root, "rail").await;
        assert_eq!(
            (v.prefix, v.value),
            (Prefix::Milli.into(), NumericValue::U16(1800))
        );
        // 3.3 V in µV is in, one µV more is not
        v.prefix = Prefix::Micro.into();
        v.value = NumericValue::U32(3_300_000);
        rail.write(client.cmd(), &v.to_value(&ty, root.bundle()).unwrap())
            .await
            .unwrap();
        assert_eq!(values.lock().unwrap().rail.to_f64(), 3.3);
        v.value = NumericValue::U32(3_300_001);
        assert!(
            rail.write(client.cmd(), &v.to_value(&ty, root.bundle()).unwrap())
                .await
                .is_err()
        );
        assert_eq!(values.lock().unwrap().rail.to_f64(), 3.3);
    }

    /// On the wire a `Fixed` is its number: the dynamic client reads and writes it as one.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fixed_values_are_numbers() {
        let (device, values) = start("quantities/fixed_values");
        let (client, root) = connect(&device).await;
        // the generic decoder shows a transparent struct as the struct of its one field (DynQuantity shows a quantity)
        let fixed = |v: ValueOwned| ValueOwned::Struct {
            fields: FieldsValueOwned::Named(vec![("value".into(), v)]),
        };
        let (value, _) = read(&client, &root, "timeout").await;
        assert_eq!(value, fixed(ValueOwned::Numeric(NumericValue::U16(250))));
        let (value, _) = read(&client, &root, "window").await;
        assert_eq!(value, fixed(ValueOwned::Numeric(NumericValue::U16(100))));
        root.child("timeout_full")
            .unwrap()
            .write(
                client.cmd(),
                &fixed(ValueOwned::Numeric(NumericValue::U16(1500))),
            )
            .await
            .unwrap();
        assert_eq!(values.lock().unwrap().timeout, Fixed::new(1500));
        assert_eq!(values.lock().unwrap().timeout.to_string(), "1500 ms");

        for (name, expected) in [
            (
                "sizes",
                sizes().iter().map(|s| s.value.get()).collect::<Vec<_>>(),
            ),
            ("ticks", ticks().iter().map(|s| s.value.get()).collect()),
        ] {
            let Some(ValueOwned::Vec(items)) = root
                .child(name)
                .unwrap()
                .call(client.cmd(), &[])
                .await
                .unwrap()
            else {
                panic!("{name}: items");
            };
            let got: Vec<u32> = items
                .iter()
                .map(|item| {
                    let ValueOwned::Struct {
                        fields: FieldsValueOwned::Named(fields),
                    } = item
                    else {
                        panic!("Timed");
                    };
                    let ValueOwned::Struct {
                        fields: FieldsValueOwned::Named(value),
                    } = &fields[1].1
                    else {
                        panic!("{name}: {:?}", fields[1].1);
                    };
                    let ValueOwned::Numeric(NumericValue::U32(v)) = value[0].1 else {
                        panic!("{name}: {:?}", value[0].1);
                    };
                    v
                })
                .collect();
            assert_eq!(got, expected, "{name}");
        }
        let _: Micros<u32> = Fixed::<units::Second, Micro, u32>::new(0);
    }

    /// CLT-54: a `Fixed` property read as a quantity, shown as the typed `Fixed` shows itself, whatever its scale (an
    /// SI prefix, a binary one, a full-scale step, a scale of the API's crate), its storage and bounds known.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fixed_shown_as_typed() {
        let (device, _values) = start("quantities/fixed_shown");
        let (client, root) = connect(&device).await;
        let typed: [(&str, String, String); 7] = [
            (
                "timeout",
                Timeout::new(250).to_string(),
                format!("{:#}", Timeout::new(250)),
            ),
            (
                "window",
                Window::bounded(100).to_string(),
                format!("{:#}", Window::bounded(100)),
            ),
            (
                "adc",
                Adc::new(2048).to_string(),
                format!("{:#}", Adc::new(2048)),
            ),
            (
                "gyro",
                Gyro::new(-100).to_string(),
                format!("{:#}", Gyro::new(-100)),
            ),
            (
                "belt",
                Belt::new(1500).to_string(),
                format!("{:#}", Belt::new(1500)),
            ),
            (
                "uptime",
                values().uptime.to_string(),
                format!("{:#}", values().uptime),
            ),
            (
                "buffer",
                values().buffer.to_string(),
                format!("{:#}", values().buffer),
            ),
        ];
        for (name, shown, alternate) in typed {
            let q = read_quantity(&client, &root, name).await;
            assert_eq!(q.to_string(), shown, "{name}");
            assert_eq!(format!("{q:#}"), alternate, "{name}");
        }
        // more than f64 holds, shown exactly
        let uptime = read_quantity(&client, &root, "uptime").await;
        assert_eq!(uptime.to_string(), "1730000000000000 µs");
        assert_eq!(format!("{uptime:#}"), "1730000000 s");
        assert_eq!(uptime.value, NumericValue::U64(1_730_000_000_000_000));
        assert_eq!(uptime.storage.as_ref().unwrap().name(), "u64");
        let buffer = read_quantity(&client, &root, "buffer").await;
        assert_eq!(
            (buffer.to_string(), format!("{buffer:#}")),
            ("4 KiB".into(), "4096 B".into())
        );
        assert_eq!(buffer.prefix.name(), "Kibi");
        assert_eq!(buffer.prefix.magnitude(), Some(Magnitude::pow2(10)));
        // information's coherent unit is the bit
        assert_eq!(buffer.to_si_f64(), Some(4096.0 * 8.0));
        let window = read_quantity(&client, &root, "window").await;
        assert_eq!(window.prefix, DynPrefix::Si(Prefix::Milli));
        assert_eq!(window.storage.unwrap().bounds.to_string(), "1..=5000");
        let (_, ty) = read(&client, &root, "adc").await;
        assert!(is_quantity(&ty, root.bundle()));
    }

    /// CLT-54: text written into a `Fixed` converts exactly into its scale and storage, or is refused naming the
    /// type; a plain number is the stored number.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fixed_written_exactly() {
        use wire_weaver_client::expr::parse_value;
        let (device, values) = start("quantities/fixed_write");
        let (client, root) = connect(&device).await;
        let write = async |name: &str, text: &str| {
            let (_, ty) = read(&client, &root, name).await;
            let value = parse_value(text, &ty, root.bundle())?;
            root.child(name)
                .unwrap()
                .write(client.cmd(), &value)
                .await
                .unwrap();
            anyhow::Ok(read_quantity(&client, &root, name).await)
        };
        let uptime = || values.lock().unwrap().uptime.get();
        assert_eq!(
            write("uptime", "q!(5 ms)").await.unwrap().to_string(),
            "5000 µs"
        );
        assert_eq!(uptime(), 5000);
        write("uptime", "q!(0.1 ms)").await.unwrap();
        assert_eq!(uptime(), 100, "a decimal as written, not as a float");
        write("uptime", "q!(1730000000 s)").await.unwrap();
        assert_eq!(uptime(), 1_730_000_000_000_000);
        // a plain number is what the field stores
        write("uptime", "5000").await.unwrap();
        assert_eq!(uptime(), 5000);
        let err = write("uptime", "q!(1.5 ns)").await.unwrap_err().to_string();
        assert_eq!(
            err,
            "1.5 ns can't be written into Fixed<Second, Micro, u64, Unbounded>: not a whole number of µs"
        );
        let err = write("timeout", "q!(70 s)").await.unwrap_err().to_string();
        assert_eq!(
            err,
            "70 s can't be written into Fixed<Second, Milli, u16, Unbounded>: 70000 is out of range for u16"
        );
        let err = write("window", "q!(6 s)").await.unwrap_err().to_string();
        assert_eq!(
            err,
            "6 s can't be written into Fixed<Second, Milli, u16, Range<1, 5000>>: 6000 is out of 1..=5000"
        );
        let err = write("window", "0").await.unwrap_err().to_string();
        assert!(err.contains("0 is out of"), "{err}");
        // binary prefixes, and a unit of the same dimension that converts exactly
        assert_eq!(
            write("buffer", "q!(1 MiB)").await.unwrap().to_string(),
            "1024 KiB"
        );
        assert_eq!(
            write("buffer", "q!(8192 B)").await.unwrap().to_string(),
            "8 KiB"
        );
        assert_eq!(values.lock().unwrap().buffer.get(), 8);
        let err = write("buffer", "q!(1000 B)").await.unwrap_err().to_string();
        assert!(err.ends_with("not a whole number of KiB"), "{err}");
        let err = write("buffer", "q!(4 V)").await.unwrap_err().to_string();
        assert!(err.contains("expected a quantity in Byte"), "{err}");
        // a quantity read from one Fixed written into another of the same unit
        let (_, timeout) = read(&client, &root, "timeout").await;
        let q = read_quantity(&client, &root, "uptime").await;
        let value = q.to_value(&timeout, root.bundle()).unwrap();
        root.child("timeout")
            .unwrap()
            .write(client.cmd(), &value)
            .await
            .unwrap();
        assert_eq!(values.lock().unwrap().timeout, Timeout::new(5));
    }

    /// CLT-54: a `Fixed` as a method's argument and return value.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fixed_method_argument() {
        use wire_weaver_client::expr::parse_value;
        let (device, values) = start("quantities/fixed_method");
        let (client, root) = connect(&device).await;
        let method = root.child("wait").unwrap();
        let ApiItemKindOwned::Method {
            args,
            return_ty: Some(return_ty),
            ..
        } = &method.item().unwrap().kind
        else {
            panic!("a method");
        };
        let bundle = root.bundle();
        let arg = parse_value("q!(2.5 ms)", &args[0].ty, bundle).unwrap();
        let ret = method.call(client.cmd(), &[arg]).await.unwrap().unwrap();
        assert_eq!(values.lock().unwrap().delay, Some(Delay::bounded(2500)));
        let ret = DynQuantity::from_value(&ret, return_ty, bundle).unwrap();
        assert_eq!(ret.to_string(), "2 ms");
        assert_eq!(ret.to_string(), Waited::new(2).to_string());
        // checked before it is sent: past the bounds
        let err = parse_value("q!(6 s)", &args[0].ty, bundle)
            .unwrap_err()
            .to_string();
        assert!(err.ends_with("6000000 is out of 0..=5000000"), "{err}");
    }

    /// CLT-54: a `Fixed` stream item.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fixed_stream_item() {
        let (device, _values) = start("quantities/fixed_stream");
        let (client, root) = connect(&device).await;
        let samples = root.child("samples").unwrap();
        let mut stream = samples.stream(client.cmd()).await.unwrap();
        let mut scratch = [0u8; 128];
        let reading = Fixed::<units::Volt, Milli, i16>::new(-1200);
        let event = server::api_impl::stream_data_ser()
            .samples(&reading, &mut scratch)
            .unwrap();
        device.send(event);
        let value = stream.recv().await.unwrap();
        let ApiItemKindOwned::Stream { ty, .. } = &samples.item().unwrap().kind else {
            panic!("a stream");
        };
        let q = DynQuantity::from_value(&value, ty, root.bundle()).unwrap();
        assert_eq!(q.to_string(), "-1200 mV");
        assert_eq!(q.to_string(), reading.to_string());
        assert_eq!(format!("{q:#}"), "-1.2 V");
    }

    /// CLT-54: indices and sequence numbers shown with what they count; writes checked as the device checks them.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn indices_and_sequence_numbers() {
        use wire_weaver_client::expr::parse_value;
        let (device, values) = start("quantities/counts");
        let (client, root) = connect(&device).await;
        let count = async |name: &str| {
            let (value, ty) = read(&client, &root, name).await;
            DynCount::from_value(&value, &ty, root.bundle()).expect("a count")
        };
        let input = count("input").await;
        assert_eq!(
            (input.kind, input.to_string()),
            (CountKind::IndexFrom1, "#2 (Item, from 1)".into())
        );
        assert_eq!(count("sample").await.to_string(), "#1000 (Sample)");
        assert_eq!(count("frame").await.to_string(), "seq 41 (Frame)");
        let write = async |name: &str, text: &str| {
            let (_, ty) = read(&client, &root, name).await;
            let value = parse_value(text, &ty, root.bundle())?;
            root.child(name)
                .unwrap()
                .write(client.cmd(), &value)
                .await
                .unwrap();
            anyhow::Ok(())
        };
        write("input", "4").await.unwrap();
        assert_eq!(values.lock().unwrap().input.get(), 4);
        let err = write("input", "0").await.unwrap_err().to_string();
        assert!(err.ends_with("it counts from 1"), "{err}");
        let err = write("input", "5").await.unwrap_err().to_string();
        assert!(err.contains("5 is out of"), "{err}");
        write("frame", "65535").await.unwrap();
        assert_eq!(values.lock().unwrap().frame.get(), 65535);
        assert!(write("frame", "65536").await.is_err());
        write("sample", "7").await.unwrap();
        assert_eq!(values.lock().unwrap().sample.get(), 7);
        let (_, ty) = read(&client, &root, "sample").await;
        assert!(!is_quantity(&ty, root.bundle()), "an index is no quantity");
    }
}

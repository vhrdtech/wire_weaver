//! `ww_si` quantities through the dynamic client (CLT-31): values read from a device over the in-process transport,
//! recognized from introspection data alone and shown with their units, the same text as the typed `Display`.

#[cfg(test)]
mod tests {
    use quantities_api::Rpm;
    use std::sync::{Arc, Mutex};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver_client::quantity::{DynQuantity, DynUnit, is_quantity, unit_of};
    use wire_weaver_client::ww_self::{ApiItemKindOwned, TypeOwned, ValueOwned};
    use wire_weaver_client::{ClientConfig, DynClient, DynResource};
    use ww_si::units::{self, AngularVelocity, Per, Torque};
    use ww_si::ww_numeric::NumericValue;
    use ww_si::{
        Ampere, AnyQuantity, Dim, Prefix, Quantity, SIExpr, Scale, Second, Timed, Unit, Volt,
    };

    type Damping = Quantity<Per<Torque, AngularVelocity>>;

    struct Values {
        voltage: Volt,
        damping: Damping,
        speed: Quantity<Rpm>,
        /// The `AnyQuantity` property, serialized.
        sensor: Vec<u8>,
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
            damping: Damping::from_f32(1.2e-4),
            speed: Quantity::<Rpm>::base(1500u16),
            sensor: sensor().to_ww_bytes(&mut [0u8; 64]).unwrap().to_vec(),
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

    mod server {
        use super::*;
        use tests_common::TestProcessEvents;
        use wire_weaver::shrink_wrap::Series;
        use wire_weaver::{GetResult, SetResult};

        pub struct Server {
            pub values: Arc<Mutex<Values>>,
            pub history: Vec<Timed<Ampere>>,
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
            (v.prefix, v.value),
            (Prefix::Milli, NumericValue::U16(3300))
        );
        assert_eq!(v.to_string(), "3300 mV");
        assert_eq!(format!("{v:#}"), "3.3 V");
        assert_eq!(v.to_string(), values().voltage.to_string());
        assert_eq!(format!("{v:#}"), format!("{:#}", values().voltage));
        assert_eq!(v.to_si_f64(), Some(3.3));
        assert_eq!(v.unit.dim().unwrap().to_string(), "kg·m²·s⁻³·A⁻¹");
        assert_eq!(v.unit.name(), "Volt");
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
        assert_eq!(b.unit.dim().unwrap().to_string(), "kg·m²·s⁻¹");
        assert_eq!(b.to_si_f64(), Some(values().damping.to_si_f64()));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unknown_marker_by_name_and_docs() {
        let (device, _values) = start("quantities/rpm");
        let (client, root) = connect(&device).await;
        let speed = read_quantity(&client, &root, "speed").await;
        assert_eq!(
            speed.unit,
            DynUnit::Unknown {
                name: "Rpm".into(),
                docs: vec!["Revolutions per minute".into()],
            }
        );
        assert_eq!(speed.to_string(), "1500 Rpm");
        assert_eq!(speed.unit.dim(), None);
        assert_eq!(speed.to_si_f64(), None);
        assert_eq!(speed.to_f64(), 1500.0);
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
        v.prefix = Prefix::Micro;
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
        assert_eq!((v.prefix, v.value), (Prefix::Kilo, NumericValue::F32(3.3)));
        let v = write("voltage", "5").await.unwrap();
        assert_eq!((v.prefix, v.value), (Prefix::Unit, NumericValue::I32(5)));
        let b = write("damping", "q!(1.2e-4 N*m/(rad/s))").await.unwrap();
        assert_eq!(b.to_string(), "0.00012 N·m/(rad/s)");
        let s = write("sensor", r#"q!("20 mA")"#).await.unwrap();
        assert_eq!((s.prefix, s.unit.symbol().as_str()), (Prefix::Milli, "A"));

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
                dim: Dim::new(-2, -1, 1, 0, 0, 0, 0),
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

        // a unit known by name only has nothing to send
        let speed = read_quantity(&client, &root, "speed").await;
        assert_eq!(
            speed.to_value(&sensor_ty, bundle).unwrap_err(),
            "a quantity in Rpm has no dimension and scale to send in an AnyQuantity"
        );
    }
}

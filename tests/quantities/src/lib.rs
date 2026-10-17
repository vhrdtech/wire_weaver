//! `ww_si` quantities through the dynamic client (CLT-31): values read from a device over the in-process transport,
//! recognized from introspection data alone and shown with their units, the same text as the typed `Display`.

#[cfg(test)]
mod tests {
    use quantities_api::Rpm;
    use std::sync::{Arc, Mutex};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver_client::quantity::{DynQuantity, DynUnit, unit_of};
    use wire_weaver_client::ww_self::{ApiItemKindOwned, TypeOwned, ValueOwned};
    use wire_weaver_client::{ClientConfig, DynClient, DynResource};
    use ww_si::units::{self, AngularVelocity, Per, Torque};
    use ww_si::ww_numeric::NumericValue;
    use ww_si::{Ampere, Prefix, Quantity, Second, Timed, Unit, Volt};

    type Damping = Quantity<Per<Torque, AngularVelocity>>;

    struct Values {
        voltage: Volt,
        damping: Damping,
        speed: Quantity<Rpm>,
    }

    fn values() -> Values {
        Values {
            voltage: Volt::milli(3300u16),
            damping: Damping::from_f32(1.2e-4),
            speed: Quantity::<Rpm>::base(1500u16),
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
}

//! The Mars Climate Orbiter case (STD-72) end to end: a device reports thruster impulse in pound-force seconds, a
//! navigation client wants newton seconds. Typed, through the generated client's types, and dynamic (from
//! introspection data alone): the value converts exactly or is refused, it is never the same number read in N·s.

#[cfg(test)]
mod tests {
    use customary_api::LbfSeconds;
    use std::sync::{Arc, Mutex};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver::shrink_wrap::RefBox;
    use wire_weaver_client::quantity::{DynQuantity, DynUnit};
    use wire_weaver_client::ww_self::{ApiItemKindOwned, FieldsValueOwned, TypeOwned, ValueOwned};
    use wire_weaver_client::{ClientConfig, DynClient, DynResource};
    use ww_si::prefix::{Milli, One, Pico, Pow10};
    use ww_si::units::{Gram, Newton, PoundAvdp, PoundForce, Second, Times};
    use ww_si::ww_numeric::NumericValue;
    use ww_si::{AnyQuantity, Fixed, Prefix, Quantity, SIExpr, Unit};

    type NewtonSeconds<P, T> = Fixed<Times<Newton, Second>, P, T>;
    type Impulse = Quantity<Times<PoundForce, Second>>;

    /// The impulse of one desaturation burn: 2 lbf·s.
    const BURN: LbfSeconds = Fixed::new(2000);

    struct Values {
        impulse: LbfSeconds,
        impulse_q: Impulse,
        impulse_any: Vec<u8>,
    }

    fn values() -> Values {
        let any = AnyQuantity::new(
            Prefix::Milli,
            2000i32,
            SIExpr::Mul(
                RefBox::Ref {
                    value: PoundForce::SI_EXPR,
                },
                RefBox::Ref {
                    value: Second::SI_EXPR,
                },
            ),
        );
        Values {
            impulse: BURN,
            impulse_q: Impulse::milli(2000i32),
            impulse_any: any.to_ww_bytes(&mut [0u8; 128]).unwrap().to_vec(),
        }
    }

    mod server {
        use super::*;
        use tests_common::TestProcessEvents;
        use wire_weaver::{GetResult, SetResult};

        pub struct Server {
            pub values: Arc<Mutex<Values>>,
            /// Bytes of the value `get_impulse_any` returns.
            pub any: Vec<u8>,
        }

        impl Server {
            fn set_impulse(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: LbfSeconds,
            ) -> SetResult<()> {
                self.values.lock().unwrap().impulse = value;
                SetResult::Set
            }

            fn get_impulse(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<LbfSeconds, ()> {
                GetResult::Value(self.values.lock().unwrap().impulse)
            }

            fn set_impulse_q(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Impulse,
            ) -> SetResult<()> {
                self.values.lock().unwrap().impulse_q = value;
                SetResult::Set
            }

            fn get_impulse_q(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Impulse, ()> {
                GetResult::Value(self.values.lock().unwrap().impulse_q)
            }

            fn set_impulse_any(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: AnyQuantity<'_>,
            ) -> SetResult<()> {
                self.values.lock().unwrap().impulse_any =
                    value.to_ww_bytes(&mut [0u8; 128]).unwrap().to_vec();
                SetResult::Set
            }

            fn get_impulse_any(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<AnyQuantity<'_>, ()> {
                self.any = self.values.lock().unwrap().impulse_any.clone();
                match AnyQuantity::from_ww_bytes(&self.any) {
                    Ok(q) => GetResult::Value(q),
                    Err(_) => GetResult::GetError(()),
                }
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                customary_api :: SmallForces for super::Server,
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

    fn start(path: &str) -> TestDevice {
        let _ = tracing_subscriber::fmt::try_init();
        let server = server::Server {
            values: Arc::new(Mutex::new(values())),
            any: vec![],
        };
        tests_common::start_device(
            path,
            server,
            customary_api::SMALL_FORCES_FULL_GID,
            server::api_impl::api_hash(),
        )
    }

    async fn connect(device: &TestDevice) -> (DynClient, DynResource) {
        let client = DynClient::from_config(ClientConfig::new().in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");
        let bundle = client.device_introspect().unwrap().api_bundle.clone();
        (client, DynResource::root(bundle))
    }

    async fn read(client: &DynClient, root: &DynResource, name: &str) -> (ValueOwned, TypeOwned) {
        let property = root.child(name).unwrap();
        let value = property.read(client.cmd()).await.unwrap();
        let ApiItemKindOwned::Property { ty, .. } = &property.item().unwrap().kind else {
            panic!("property");
        };
        (value, ty.clone())
    }

    fn n_s() -> DynUnit {
        DynUnit::from_name("Times<Newton, Second>").unwrap()
    }

    /// The typed side: what a Rust client holding the API's types does.
    #[test]
    fn typed_converts_exactly() {
        // 2 lbf·s = 8.896443230521 N·s, exactly
        assert_eq!(
            BURN.convert::<Times<Newton, Second>, Pico, i64>(),
            Some(NewtonSeconds::new(8_896_443_230_521))
        );
        // not a whole number of mN·s: refused, not rounded
        assert_eq!(BURN.convert::<Times<Newton, Second>, Milli, i64>(), None);
        let f = BURN
            .convert::<Times<Newton, Second>, One, f64>()
            .unwrap()
            .get();
        assert!((f - 8.896443230521).abs() < 1e-12);
        // the 1999 error: the number read as N·s is 4.45 times too small
        assert!((f / 2.0 - 4.4482216152605).abs() < 1e-12);
        // back: 8.896443230521 N·s is 2000 mlbf·s
        assert_eq!(
            NewtonSeconds::<Pow10<-12>, i64>::new(8_896_443_230_521)
                .convert::<Times<PoundForce, Second>, Milli, i32>(),
            Some(BURN)
        );
        // the units' own factors, exact
        assert_eq!(
            format!("{:#}", BURN),
            "2 lbf·s",
            "shown in its own unit, never as N·s"
        );
        assert_eq!(
            <PoundForce as Unit>::EXPR.magnitude(),
            <PoundAvdp as Unit>::EXPR
                .magnitude()
                .unwrap()
                .checked_mul(ww_si::Magnitude::new(980665, 1, -5, 0, 0))
                .ok()
        );
        assert_ne!(PoundForce::DIM, PoundAvdp::DIM);
        assert_eq!(PoundAvdp::DIM, Gram::DIM);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn generated_types_carry_the_unit() {
        let device = start("customary/fixed");
        let (client, root) = connect(&device).await;
        let (value, ty) = read(&client, &root, "impulse").await;
        // a Fixed is on the wire exactly its number: the dynamic client sees 2000 and the type names the unit
        let ValueOwned::Struct {
            fields: FieldsValueOwned::Named(fields),
        } = &value
        else {
            panic!("{value:?}");
        };
        assert_eq!(
            fields.as_slice(),
            [(
                "value".to_string(),
                ValueOwned::Numeric(NumericValue::I32(2000))
            )]
        );
        let name = ty.human_name(false, root.bundle()).unwrap();
        assert!(name.contains("PoundForce"), "{name}");
        // TODO(CLT-54): a Fixed field shown and converted with its unit in dynamic clients, Python and C
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dynamic_quantity_converts_or_refuses() {
        let device = start("customary/quantity");
        let (client, root) = connect(&device).await;
        for name in ["impulse_q", "impulse_any"] {
            let (value, ty) = read(&client, &root, name).await;
            let q = DynQuantity::from_value(&value, &ty, root.bundle()).expect("a quantity");
            assert_eq!(q.unit.name(), "Times<PoundForce, Second>", "{name}");
            assert_eq!(q.to_string(), "2000 m(lbf·s)", "{name}");
            let si = q.convert(&n_s()).unwrap();
            assert_eq!(
                (si.prefix, si.value),
                (Prefix::Pico, NumericValue::I64(8_896_443_230_521)),
                "{name}"
            );
            assert_eq!(si.to_string(), "8896443230521 p(N·s)");
            assert!((si.to_si_f64().unwrap() - 8.896443230521).abs() < 1e-12);
            // a pound of mass times a second is not an impulse
            let lb_s = DynUnit::from_name("Times<PoundAvdp, Second>").unwrap();
            let err = q.convert(&lb_s).unwrap_err();
            assert!(err.contains("dimension"), "{err}");
            // and back, exactly
            assert_eq!(si.convert(&q.unit).unwrap().to_string(), "2 lbf·s");
        }
    }

    #[test]
    fn dynamic_refuses_what_is_not_exact() {
        let psi = DynQuantity {
            prefix: Prefix::Unit,
            value: NumericValue::I32(1),
            unit: DynUnit::from_name("Psi").unwrap(),
        };
        let pa = DynUnit::from_name("Pascal").unwrap();
        // 6894.757293168361.. Pa: no prefix holds it whole
        let err = psi.convert(&pa).unwrap_err();
        assert!(err.contains("no SI prefix"), "{err}");
        let f = DynQuantity {
            value: NumericValue::F64(1.0),
            ..psi
        };
        assert!((f.convert(&pa).unwrap().to_f64() - 6894.757293168361).abs() < 1e-9);
        // names round-trip
        for name in [
            "Times<PoundForce, Second>",
            "Per<Meter, Second>",
            "Pow<Inch, 3>",
            "Sqrt<Hertz>",
        ] {
            assert_eq!(DynUnit::from_name(name).unwrap().name(), name);
        }
        assert!(
            DynUnit::from_name("Pound").is_err(),
            "no bare ambiguous name"
        );
        assert!(
            DynUnit::from_name("Gallon").is_err(),
            "no bare ambiguous name"
        );
    }
}

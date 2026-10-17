//! Generic user types in an API (API-43), through generated server and client code: method arguments and return
//! values, properties and stream items; their definitions in introspection data (each generic definition once, a use
//! is an instance with its arguments, `ww_si::Timed` left out as known from snapshots), and the dynamic client, which
//! decodes and writes the same bytes as the derived types. Type aliases (API-45) are what they name, in generated
//! code, introspection data and the dynamic client.

#[cfg(test)]
mod tests {
    use generics_api::{Outer, Pair, Quantity, Reading, Sample, TaggedOwned, Volt, Window};
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver_client::ww_numeric::NumericValue;
    use wire_weaver_client::ww_self::{
        ApiBundleOwned, ApiItemKindOwned, FieldsValueOwned, TypeLocationOwned, TypeOwned,
        ValueOwned,
    };
    use wire_weaver_client::{ClientConfig, DynClient, DynResource};
    use ww_si::{Ampere, Prefix, Second, Timed};

    #[derive(Default)]
    struct SharedTestData {
        latest: Option<Timed<Ampere>>,
        outer: Option<Outer<3>>,
        voltage: Option<Volt>,
        timed_voltage: Option<Timed<Volt>>,
        sample: Option<Sample<u8>>,
    }

    mod server {
        use super::*;
        use generics_api::Tagged;
        use tests_common::TestProcessEvents;
        use wire_weaver::{GetResult, SetResult};

        pub struct Server {
            pub data: Arc<Mutex<SharedTestData>>,
        }

        impl Server {
            fn swap(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                pair: Pair<u8, u16>,
            ) -> RpcResult<Pair<u16, u8>> {
                Ready(Pair {
                    a: pair.b,
                    b: pair.a,
                })
            }

            fn tagged(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                tagged: Tagged<'_, Reading<u32>>,
            ) -> RpcResult<Pair<u32, Reading<u32>>> {
                Ready(Pair {
                    a: tagged.tag.len() as u32,
                    b: tagged.value,
                })
            }

            fn sum(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                window: Window<4>,
            ) -> RpcResult<u32> {
                Ready(window.samples.iter().map(|&s| s as u32).sum())
            }

            fn set_latest(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Timed<Ampere>,
            ) -> SetResult<()> {
                self.data.lock().unwrap().latest = Some(value);
                SetResult::Set
            }

            fn get_latest(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Timed<Ampere>, ()> {
                match self.data.lock().unwrap().latest {
                    Some(latest) => GetResult::Value(latest),
                    None => GetResult::GetError(()),
                }
            }

            fn set_outer(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Outer<3>,
            ) -> SetResult<()> {
                self.data.lock().unwrap().outer = Some(value);
                SetResult::Set
            }

            fn get_outer(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Outer<3>, ()> {
                match self.data.lock().unwrap().outer {
                    Some(outer) => GetResult::Value(outer),
                    None => GetResult::GetError(()),
                }
            }

            fn readings(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
                Ready(())
            }

            fn double(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                v: Volt,
            ) -> RpcResult<Volt> {
                Ready(Quantity::new(v.value * 2))
            }

            fn set_voltage(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Volt,
            ) -> SetResult<()> {
                self.data.lock().unwrap().voltage = Some(value);
                SetResult::Set
            }

            fn get_voltage(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Volt, ()> {
                match self.data.lock().unwrap().voltage {
                    Some(v) => GetResult::Value(v),
                    None => GetResult::GetError(()),
                }
            }

            fn set_timed_voltage(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Timed<Volt>,
            ) -> SetResult<()> {
                self.data.lock().unwrap().timed_voltage = Some(value);
                SetResult::Set
            }

            fn get_timed_voltage(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Timed<Volt>, ()> {
                match self.data.lock().unwrap().timed_voltage {
                    Some(v) => GetResult::Value(v),
                    None => GetResult::GetError(()),
                }
            }

            fn set_sample(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Sample<u8>,
            ) -> SetResult<()> {
                self.data.lock().unwrap().sample = Some(value);
                SetResult::Set
            }

            fn get_sample(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Sample<u8>, ()> {
                match self.data.lock().unwrap().sample {
                    Some(v) => GetResult::Value(v),
                    None => GetResult::GetError(()),
                }
            }

            fn close_readings(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn sideband_readings(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                generics_api :: Generics for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                introspect = "no_docs",
                // debug_to_file = "../../target/tests_generics_server.rs"
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
                generics_api :: Generics for super::StdClient,
                client = "std_client",
                // debug_to_file = "../../target/tests_generics_client.rs"
            );
        }
    }

    use server::api_impl;
    use std_client::StdClient;

    fn start(path: &str) -> (TestDevice, Arc<Mutex<SharedTestData>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let data = Arc::new(Mutex::new(SharedTestData::default()));
        let server = server::Server { data: data.clone() };
        let device = tests_common::start_device(
            path,
            server,
            generics_api::GENERICS_FULL_GID,
            api_impl::api_hash(),
        );
        (device, data)
    }

    async fn connect(device: &TestDevice) -> StdClient {
        StdClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    fn latest() -> Timed<Ampere> {
        Timed::new(
            Second {
                prefix: Prefix::Milli,
                value: NumericValue::U32(1500),
            },
            Ampere {
                prefix: Prefix::Micro,
                value: NumericValue::I16(-250),
            },
        )
    }

    fn outer() -> Outer<3> {
        Outer {
            id: 7,
            window: Window {
                samples: [1, 2, 0xfff],
            },
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn arguments_and_return_values() {
        let (device, _data) = start("generics/methods");
        let mut client = connect(&device).await;
        let swapped = client.swap(Pair { a: 1, b: 300 }).call().await.unwrap();
        assert_eq!(swapped, Pair { a: 300, b: 1 });

        let tagged = TaggedOwned {
            tag: "abc".into(),
            value: Reading::Range { lo: 5, hi: 9 },
        };
        let r = client.tagged(tagged).call().await.unwrap();
        assert_eq!(
            r,
            Pair {
                a: 3,
                b: Reading::Range { lo: 5, hi: 9 }
            }
        );

        let window = Window {
            samples: [1, 2, 3, 400],
        };
        assert_eq!(client.sum(window).call().await.unwrap(), 406);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn properties() {
        let (device, data) = start("generics/properties");
        let client = connect(&device).await;
        client.write_latest(latest()).write().await.unwrap();
        let read = client.read_latest().read().await.unwrap();
        assert_eq!(format!("{read:?}"), format!("{:?}", latest()));
        assert!(data.lock().unwrap().latest.is_some());

        client.write_outer(outer()).write().await.unwrap();
        assert_eq!(client.read_outer().read().await.unwrap(), outer());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stream() {
        let (device, _data) = start("generics/stream");
        let client = connect(&device).await;
        let mut readings = client.readings().call().await.unwrap();
        let mut scratch = [0u8; 64];
        for reading in [
            generics_api::Reading::Value(-3i16),
            generics_api::Reading::Missing,
        ] {
            device.send(
                api_impl::stream_data_ser()
                    .readings(&reading, &mut scratch)
                    .unwrap(),
            );
        }
        assert_eq!(readings.recv().await.unwrap(), Reading::Value(-3));
        assert_eq!(readings.recv().await.unwrap(), Reading::Missing);
    }

    /// The API model as codegen sees it, generic definitions with parameters, uses as instances.
    fn bundle() -> ApiBundleOwned {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../generics_api");
        wire_weaver_core::load(&path, Some("Generics".into()), false).unwrap()
    }

    fn generic_defs(bundle: &ApiBundleOwned) -> Vec<String> {
        bundle
            .types
            .iter()
            .filter_map(|location| match location {
                TypeLocationOwned::InLine {
                    ty: TypeOwned::Struct(s),
                    ..
                } if !s.generics.is_empty() => Some(s.ident.clone()),
                TypeLocationOwned::InLine {
                    ty: TypeOwned::Enum(e),
                    ..
                } if !e.generics.is_empty() => Some(e.ident.clone()),
                _ => None,
            })
            .collect()
    }

    fn method_types(bundle: &ApiBundleOwned, id: u32) -> (TypeOwned, TypeOwned) {
        let ApiItemKindOwned::Method {
            args, return_ty, ..
        } = &bundle.get_item(&[id]).unwrap().kind
        else {
            panic!("method");
        };
        (args[0].ty.clone(), return_ty.clone().unwrap())
    }

    #[test]
    fn generic_definitions_once_uses_as_instances() {
        let bundle = bundle();
        let mut defs = generic_defs(&bundle);
        defs.sort();
        assert_eq!(
            defs,
            [
                "Outer", "Pair", "Quantity", "Reading", "Tagged", "Timed", "Window"
            ]
        );
        let (arg, ret) = method_types(&bundle, 0);
        assert!(matches!(arg, TypeOwned::Instance { .. }));
        assert_eq!(arg.human_name(false, &bundle).unwrap(), "Pair<u8, u16>");
        assert_eq!(
            ret.human_name(true, &bundle).unwrap(),
            "generics_api::Pair<u16, u8>"
        );
        let (arg, ret) = method_types(&bundle, 1);
        assert_eq!(
            arg.human_name(false, &bundle).unwrap(),
            "Tagged<Reading<u32>>"
        );
        assert_eq!(
            ret.human_name(false, &bundle).unwrap(),
            "Pair<u32, Reading<u32>>"
        );
        let (arg, _) = method_types(&bundle, 2);
        assert_eq!(arg.human_name(false, &bundle).unwrap(), "Window<4>");
        assert_eq!(
            arg.human_definition(&bundle, true).unwrap(),
            "struct generics_api::Window<4> {samples: [u16; 4]}"
        );
    }

    fn timed_voltage() -> Timed<Volt> {
        Timed::new(
            Second {
                prefix: Prefix::Micro,
                value: NumericValue::U16(20),
            },
            Quantity::new(-5),
        )
    }

    fn sample() -> Sample<u8> {
        Pair {
            a: Window { samples: [3, 4] },
            b: Reading::Value(9),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn type_aliases() {
        let (device, data) = start("generics/aliases");
        let mut client = connect(&device).await;
        let doubled = client.double(Quantity::new(21)).call().await.unwrap();
        assert_eq!(doubled, Quantity::new(42));

        client
            .write_voltage(Quantity::new(3300))
            .write()
            .await
            .unwrap();
        assert_eq!(
            client.read_voltage().read().await.unwrap(),
            Quantity::new(3300)
        );
        client
            .write_timed_voltage(timed_voltage())
            .write()
            .await
            .unwrap();
        let read = client.read_timed_voltage().read().await.unwrap();
        assert_eq!(format!("{read:?}"), format!("{:?}", timed_voltage()));
        client.write_sample(sample()).write().await.unwrap();
        assert_eq!(client.read_sample().read().await.unwrap(), sample());
        assert_eq!(data.lock().unwrap().sample, Some(sample()));
    }

    /// Item `id` of the root: a property's type or a method's first argument.
    fn item_ty(bundle: &ApiBundleOwned, id: u32) -> TypeOwned {
        match &bundle.get_item(&[id]).unwrap().kind {
            ApiItemKindOwned::Property { ty, .. } => ty.clone(),
            ApiItemKindOwned::Method { args, .. } => args[0].ty.clone(),
            _ => panic!("property or method"),
        }
    }

    #[test]
    fn type_aliases_are_what_they_name() {
        let bundle = bundle();
        let (arg, ret) = method_types(&bundle, 6);
        assert_eq!(arg, ret);
        assert_eq!(arg, item_ty(&bundle, 7));
        assert_eq!(
            arg.human_name(false, &bundle).unwrap(),
            "Quantity<VoltUnit>"
        );
        assert_eq!(
            item_ty(&bundle, 8).human_name(false, &bundle).unwrap(),
            "Timed<Quantity<VoltUnit>>"
        );
        assert_eq!(
            item_ty(&bundle, 9).human_definition(&bundle, true).unwrap(),
            "struct generics_api::Pair<Window<2>, Reading<u8>> {a: struct generics_api::Window<2> {samples: [u16; 2]}, \
             b: enum ub4 generics_api::Reading<u8> {Missing, Value(u8), Range{lo: u8, hi: u8}}}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn introspection_leaves_known_definitions_out() {
        let (device, _data) = start("generics/introspect");
        let client = DynClient::from_config(ClientConfig::new().in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");
        let introspect = client.device_introspect().unwrap();
        // as sent: Timed is known from the ww_si snapshot, left out, its use is an instance
        let sent = &introspect.sent_api_bundle;
        assert!(sent.types.iter().any(|l| matches!(
            l,
            TypeLocationOwned::SkippedFullVersion { type_name, .. } if type_name == "Timed"
        )));
        assert!(!generic_defs(sent).contains(&"Timed".to_string()));
        // put back and instantiated: a concrete definition per instance
        let bundle = &introspect.api_bundle;
        let ApiItemKindOwned::Property { ty, .. } = &bundle.get_item(&[3]).unwrap().kind else {
            panic!("property");
        };
        assert!(matches!(ty, TypeOwned::OutOfLine { .. }));
        assert_eq!(ty.human_name(false, bundle).unwrap(), "Timed<Ampere>");
        let ApiItemKindOwned::Property { ty, .. } = &bundle.get_item(&[4]).unwrap().kind else {
            panic!("property");
        };
        assert_eq!(
            ty.human_definition(bundle, true).unwrap(),
            "struct generics_api::Outer<3> {id: u8, window: struct generics_api::Window<3> {samples: [u16; 3]}}"
        );
    }

    fn num(v: NumericValue) -> ValueOwned {
        ValueOwned::Numeric(v)
    }

    fn named(fields: Vec<(&str, ValueOwned)>) -> ValueOwned {
        ValueOwned::Struct {
            fields: FieldsValueOwned::Named(
                fields
                    .into_iter()
                    .map(|(n, v)| (n.to_string(), v))
                    .collect(),
            ),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dynamic_client() {
        let (device, data) = start("generics/dynamic");
        let client = DynClient::from_config(ClientConfig::new().in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");
        let cmd = client.cmd();
        let bundle = client.device_introspect().unwrap().api_bundle.clone();
        let root = DynResource::root(bundle.clone());

        let pair = named(vec![
            ("a", num(NumericValue::U8(1))),
            ("b", num(NumericValue::U16(300))),
        ]);
        let r = root
            .child("swap")
            .unwrap()
            .call(cmd, &[pair])
            .await
            .unwrap();
        assert_eq!(
            r,
            Some(named(vec![
                ("a", num(NumericValue::U16(300))),
                ("b", num(NumericValue::U8(1))),
            ]))
        );

        let window = named(vec![(
            "samples",
            ValueOwned::Array([1u16, 2, 3, 4].map(|v| num(NumericValue::U16(v))).to_vec()),
        )]);
        let r = root
            .child("sum")
            .unwrap()
            .call(cmd, &[window])
            .await
            .unwrap();
        assert_eq!(r, Some(num(NumericValue::U32(10))));

        // written by the generated client, read dynamically: the same bytes
        data.lock().unwrap().outer = Some(outer());
        let value = root.child("outer").unwrap().read(cmd).await.unwrap();
        let expected = named(vec![
            ("id", num(NumericValue::U8(7))),
            (
                "window",
                named(vec![(
                    "samples",
                    ValueOwned::Array([1u16, 2, 0xfff].map(|v| num(NumericValue::U16(v))).to_vec()),
                )]),
            ),
        ]);
        assert_eq!(value, expected);
        let ApiItemKindOwned::Property { ty, .. } = &bundle.get_item(&[4]).unwrap().kind else {
            panic!("property");
        };
        assert_eq!(
            value.ser_shrink_wrap_dyn(ty, &bundle).unwrap(),
            outer().to_ww_bytes_owned().unwrap()
        );

        // Timed<Ampere> from the snapshot definitions
        let latest_value = named(vec![
            (
                "t",
                named(vec![
                    (
                        "prefix",
                        ValueOwned::Enum {
                            variant: "Milli".into(),
                            fields: FieldsValueOwned::Unit,
                        },
                    ),
                    (
                        "value",
                        ValueOwned::Enum {
                            variant: "U32".into(),
                            fields: FieldsValueOwned::Unnamed(vec![num(NumericValue::U32(1500))]),
                        },
                    ),
                ]),
            ),
            (
                "value",
                named(vec![
                    (
                        "prefix",
                        ValueOwned::Enum {
                            variant: "Micro".into(),
                            fields: FieldsValueOwned::Unit,
                        },
                    ),
                    (
                        "value",
                        ValueOwned::Enum {
                            variant: "I16".into(),
                            fields: FieldsValueOwned::Unnamed(vec![num(NumericValue::I16(-250))]),
                        },
                    ),
                ]),
            ),
        ]);
        let latest_res = root.child("latest").unwrap();
        latest_res.write(cmd, &latest_value).await.unwrap();
        let stored = data.lock().unwrap().latest;
        assert_eq!(format!("{stored:?}"), format!("{:?}", Some(latest())));
        assert_eq!(latest_res.read(cmd).await.unwrap(), latest_value);

        // stream items of a generic enum
        let mut readings = root.child("readings").unwrap().stream(cmd).await.unwrap();
        let mut scratch = [0u8; 64];
        device.send(
            api_impl::stream_data_ser()
                .readings(
                    &generics_api::Reading::Range { lo: -1i16, hi: 1 },
                    &mut scratch,
                )
                .unwrap(),
        );
        assert_eq!(
            readings.recv().await.unwrap(),
            ValueOwned::Enum {
                variant: "Range".into(),
                fields: FieldsValueOwned::Named(vec![
                    ("lo".into(), num(NumericValue::I16(-1))),
                    ("hi".into(), num(NumericValue::I16(1))),
                ]),
            }
        );

        // type aliases: a property and inside `Timed`, the same bytes as the derived types
        data.lock().unwrap().voltage = Some(Quantity::new(3300));
        let value = root.child("voltage").unwrap().read(cmd).await.unwrap();
        assert_eq!(value, named(vec![("value", num(NumericValue::I32(3300)))]));
        data.lock().unwrap().timed_voltage = Some(timed_voltage());
        let timed_res = root.child("timed_voltage").unwrap();
        let value = timed_res.read(cmd).await.unwrap();
        let ApiItemKindOwned::Property { ty, .. } = &bundle.get_item(&[8]).unwrap().kind else {
            panic!("property");
        };
        assert_eq!(
            value.ser_shrink_wrap_dyn(ty, &bundle).unwrap(),
            timed_voltage().to_ww_bytes_owned().unwrap()
        );
        data.lock().unwrap().timed_voltage = None;
        timed_res.write(cmd, &value).await.unwrap();
        let stored = data.lock().unwrap().timed_voltage;
        assert_eq!(
            format!("{stored:?}"),
            format!("{:?}", Some(timed_voltage()))
        );
        let r = root
            .child("double")
            .unwrap()
            .call(cmd, &[named(vec![("value", num(NumericValue::I32(-4)))])])
            .await
            .unwrap();
        assert_eq!(r, Some(named(vec![("value", num(NumericValue::I32(-8)))])));
    }
}

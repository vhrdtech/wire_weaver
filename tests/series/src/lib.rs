//! Compressed sequences (`Delta`, `DeltaOfDelta`, `XorFloat`, SW-31, SW-32) in an API (SW-50), through generated
//! server and client code: arguments, return values, struct fields, properties and streams; their types in
//! introspection data and the dynamic client, which writes the same bytes as the derived types.

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver_client::ww_numeric::NumericBaseType;
    use wire_weaver_client::ww_numeric::NumericValue;
    use wire_weaver_client::ww_self::{
        ApiBundleOwned, ApiItemKindOwned, FieldsValueOwned, TypeOwned, ValueOwned,
    };
    use wire_weaver_client::{ClientConfig, DynClient, DynResource};

    #[derive(Default)]
    struct SharedTestData {
        stamps: Vec<u64>,
    }

    mod server {
        use super::*;
        use series_api::Capture;
        use tests_common::TestProcessEvents;
        use wire_weaver::{GetResult, SetResult};

        pub struct Server {
            pub data: Arc<Mutex<SharedTestData>>,
            /// What borrowed return values point into
            pub ramp: Vec<i16>,
            pub scaled: Vec<f32>,
            pub stamps: Vec<u64>,
        }

        impl Server {
            fn sum(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                timestamps: DeltaOfDelta<'_, u32>,
            ) -> RpcResult<u64> {
                Ready(timestamps.iter().map(u64::from).sum())
            }

            fn ramp(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                start: i16,
                count: u8,
            ) -> RpcResult<Delta<'_, i16>> {
                self.ramp = (0..count as i16).map(|i| start - i).collect();
                Ready(Delta::new(&self.ramp))
            }

            fn scale(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                values: XorFloat<'_, f32>,
                k: f32,
            ) -> RpcResult<XorFloat<'_, f32>> {
                self.scaled = values.iter().map(|v| v * k).collect();
                Ready(XorFloat::new(&self.scaled))
            }

            fn capture(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                capture: Capture<'_>,
            ) -> RpcResult<u32> {
                assert_eq!(capture.levels.len(), capture.timestamps.len());
                Ready(capture.timestamps.len() as u32 + capture.channel as u32 * 1000)
            }

            fn set_stamps(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: DeltaOfDelta<'_, u64>,
            ) -> SetResult<()> {
                self.data.lock().unwrap().stamps = value.iter().collect();
                SetResult::Set
            }

            fn get_stamps(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<DeltaOfDelta<'_, u64>, ()> {
                self.stamps = self.data.lock().unwrap().stamps.clone();
                GetResult::Value(DeltaOfDelta::new(&self.stamps))
            }

            fn samples(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
                Ready(())
            }

            fn close_samples(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn sideband_samples(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                series_api :: Series for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                // debug_to_file = "../../target/tests_series_server.rs"
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
                series_api :: Series for super::StdClient,
                client = "std_client",
                // debug_to_file = "../../target/tests_series_client.rs"
            );
        }
    }

    use server::api_impl;
    use std_client::StdClient;

    const TIMESTAMPS: [u32; 6] = [
        1_000_000, 1_000_100, 1_000_200, 1_000_300, 1_000_401, 1_000_500,
    ];

    fn start(path: &str) -> (TestDevice, Arc<Mutex<SharedTestData>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let data = Arc::new(Mutex::new(SharedTestData::default()));
        let server = server::Server {
            data: data.clone(),
            ramp: vec![],
            scaled: vec![],
            stamps: vec![],
        };
        let device = tests_common::start_device(
            path,
            server,
            series_api::SERIES_FULL_GID,
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

    fn bundle() -> Arc<ApiBundleOwned> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../series_api");
        Arc::new(wire_weaver_core::load(&path, Some("Series".into()), false).unwrap())
    }

    fn capture() -> series_api::CaptureOwned {
        series_api::CaptureOwned {
            channel: 3,
            timestamps: DeltaOfDeltaOwned(TIMESTAMPS.to_vec()),
            levels: XorFloatOwned(vec![1.5, 1.5, 1.25, -0.0, f64::NAN, 1e300]),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn arguments_return_values_and_fields() {
        let (device, _data) = start("series/methods");
        let mut client = connect(&device).await;

        let sum: u64 = TIMESTAMPS.iter().map(|&t| t as u64).sum();
        let timestamps = DeltaOfDeltaOwned(TIMESTAMPS.to_vec());
        assert_eq!(client.sum(timestamps).call().await.unwrap(), sum);
        assert_eq!(
            client.sum(DeltaOfDeltaOwned::new()).call().await.unwrap(),
            0
        );

        let ramp = client.ramp(-3, 4).call().await.unwrap();
        assert_eq!(ramp, DeltaOwned(vec![-3, -4, -5, -6]));

        let scaled = client
            .scale(XorFloatOwned(vec![1.0, 2.5, -4.0]), 2.0)
            .call()
            .await
            .unwrap();
        assert_eq!(scaled, XorFloatOwned(vec![2.0, 5.0, -8.0]));

        assert_eq!(client.capture(capture()).call().await.unwrap(), 3006);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn property() {
        let (device, data) = start("series/property");
        let client = connect(&device).await;
        let stamps = vec![1_700_000_000_000u64, 1_700_000_000_250, 1_700_000_000_500];
        client
            .write_stamps(DeltaOfDeltaOwned(stamps.clone()))
            .write()
            .await
            .unwrap();
        assert_eq!(data.lock().unwrap().stamps, stamps);
        assert_eq!(client.read_stamps().read().await.unwrap().0, stamps);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stream() {
        let (device, _data) = start("series/stream");
        let client = connect(&device).await;
        let mut samples = client.samples().call().await.unwrap();
        let mut scratch = [0u8; 64];
        let values = [512u16, 513, 515, 515, 0, u16::MAX];
        device.send(
            api_impl::stream_data_ser()
                .samples(&Delta::new(&values), &mut scratch)
                .unwrap(),
        );
        assert_eq!(samples.recv().await.unwrap(), DeltaOwned(values.to_vec()));
    }

    #[test]
    fn types_in_introspection() {
        let bundle = bundle();
        let ty_of_arg = |id: u32, arg: usize| {
            let ApiItemKindOwned::Method { args, .. } = &bundle.get_item(&[id]).unwrap().kind
            else {
                panic!("method");
            };
            args[arg].ty.clone()
        };
        assert_eq!(
            ty_of_arg(0, 0),
            TypeOwned::DeltaOfDelta(NumericBaseType::U32)
        );
        assert_eq!(ty_of_arg(2, 0), TypeOwned::XorFloat(NumericBaseType::F32));
        let ApiItemKindOwned::Method { return_ty, .. } = &bundle.get_item(&[1]).unwrap().kind
        else {
            panic!("method");
        };
        let return_ty = return_ty.as_ref().unwrap();
        assert_eq!(*return_ty, TypeOwned::Delta(NumericBaseType::I16));
        assert_eq!(return_ty.human_name(false, &bundle).unwrap(), "Delta<i16>");
        let ApiItemKindOwned::Property { ty, .. } = &bundle.get_item(&[4]).unwrap().kind else {
            panic!("property");
        };
        assert_eq!(ty.human_name(false, &bundle).unwrap(), "DeltaOfDelta<u64>");
    }

    #[test]
    fn dynamic_value_has_the_bytes_of_the_derived_type() {
        let bundle = bundle();
        let ty = ty_of_capture(&bundle);
        let num = ValueOwned::Numeric;
        let value = ValueOwned::Struct {
            fields: FieldsValueOwned::Named(vec![
                ("channel".into(), num(NumericValue::U8(3))),
                (
                    "timestamps".into(),
                    ValueOwned::Vec(
                        TIMESTAMPS
                            .iter()
                            .map(|&t| num(NumericValue::U32(t)))
                            .collect(),
                    ),
                ),
                (
                    "levels".into(),
                    ValueOwned::Vec(
                        [1.5, 1.5, 1.25, -0.0, f64::NAN, 1e300]
                            .into_iter()
                            .map(|v| num(NumericValue::F64(v)))
                            .collect(),
                    ),
                ),
            ]),
        };
        let derived = capture().to_ww_bytes_owned().unwrap();
        let dynamic = value.ser_shrink_wrap_dyn(&ty, &bundle).unwrap();
        assert_eq!(dynamic, derived);

        let back = ValueOwned::des_shrink_wrap_dyn(&derived, &ty, &bundle).unwrap();
        let ValueOwned::Struct {
            fields: FieldsValueOwned::Named(fields),
        } = back
        else {
            panic!("struct");
        };
        assert_eq!(fields[1], value_field(&value, 1));
        // NaN is kept bit for bit, compare the bits
        let ValueOwned::Vec(levels) = &fields[2].1 else {
            panic!("levels");
        };
        let bits: Vec<u64> = levels
            .iter()
            .map(|v| match v {
                ValueOwned::Numeric(NumericValue::F64(v)) => v.to_bits(),
                other => panic!("{other:?}"),
            })
            .collect();
        let expected: Vec<u64> = capture().levels.iter().map(|v| v.to_bits()).collect();
        assert_eq!(bits, expected);

        // out of range for the element type
        let too_big = ValueOwned::Vec(vec![num(NumericValue::U64(1 << 40))]);
        let err = too_big
            .ser_shrink_wrap_dyn(&TypeOwned::DeltaOfDelta(NumericBaseType::U32), &bundle)
            .unwrap_err()
            .to_string();
        assert!(err.contains("out of range"), "{err}");
    }

    fn ty_of_capture(bundle: &ApiBundleOwned) -> TypeOwned {
        let ApiItemKindOwned::Method { args, .. } = &bundle.get_item(&[3]).unwrap().kind else {
            panic!("method");
        };
        args[0].ty.clone()
    }

    fn value_field(value: &ValueOwned, idx: usize) -> (String, ValueOwned) {
        let ValueOwned::Struct {
            fields: FieldsValueOwned::Named(fields),
        } = value
        else {
            panic!("struct");
        };
        fields[idx].clone()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dynamic_client() {
        let (device, data) = start("series/dynamic");
        let client = DynClient::from_config(ClientConfig::new().in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");
        let cmd = client.cmd();
        let root = DynResource::root(bundle());
        let num = ValueOwned::Numeric;

        // any integer variant in range is written as u32
        let timestamps = ValueOwned::Vec(
            TIMESTAMPS
                .iter()
                .map(|&t| num(NumericValue::U64(t as u64)))
                .collect(),
        );
        let sum: u64 = TIMESTAMPS.iter().map(|&t| t as u64).sum();
        let r = root
            .child("sum")
            .unwrap()
            .call(cmd, &[timestamps])
            .await
            .unwrap();
        assert_eq!(r, Some(num(NumericValue::U64(sum))));

        let r = root
            .child("ramp")
            .unwrap()
            .call(cmd, &[num(NumericValue::I16(10)), num(NumericValue::U8(3))])
            .await
            .unwrap();
        let expected = [10i16, 9, 8].map(|v| num(NumericValue::I16(v)));
        assert_eq!(r, Some(ValueOwned::Vec(expected.to_vec())));

        let stamps = root.child("stamps").unwrap();
        let values = ValueOwned::Vec(vec![num(NumericValue::U64(5)), num(NumericValue::U64(7))]);
        stamps.write(cmd, &values).await.unwrap();
        assert_eq!(data.lock().unwrap().stamps, vec![5, 7]);
        assert_eq!(stamps.read(cmd).await.unwrap(), values);
    }
}

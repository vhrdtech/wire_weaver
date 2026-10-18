//! Compressed sequences (`Delta`, `DeltaOfDelta`, `XorFloat`, SW-31, SW-32) in an API (SW-50), through generated
//! server and client code: arguments, return values, struct fields, properties and streams; their types in
//! introspection data and the dynamic client, which writes the same bytes as the derived types.

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver::shrink_wrap::{Series, SeriesOwned};
    use wire_weaver_client::ww_numeric::NumericBaseType;
    use wire_weaver_client::ww_numeric::NumericValue;
    use wire_weaver_client::ww_self::{
        ApiBundleOwned, ApiItemKindOwned, FieldsValueOwned, TypeOwned, ValueOwned,
    };
    use wire_weaver_client::{ClientConfig, DynClient, DynResource};

    #[derive(Default)]
    struct SharedTestData {
        stamps: Vec<u64>,
        log: Vec<series_api::ReadingOwned>,
    }

    mod server {
        use super::*;
        use series_api::Capture;
        use tests_common::TestProcessEvents;
        use wire_weaver::shrink_wrap::Series;
        use wire_weaver::{GetResult, SetResult};

        pub struct Server {
            pub data: Arc<Mutex<SharedTestData>>,
            /// What borrowed return values point into
            pub ramp: Vec<i16>,
            pub scaled: Vec<f32>,
            pub stamps: Vec<u64>,
            pub readings: Vec<series_api::Reading<'static>>,
            pub history: Vec<ww_si::Timed<ww_si::Volt>>,
            /// The `Series` bytes of the last `log` written
            pub log: Vec<u8>,
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

            fn set_log<'i>(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: Series<'i, series_api::Reading<'i>>,
            ) -> SetResult<()> {
                self.log = value.to_ww_bytes_owned().unwrap();
                self.data.lock().unwrap().log =
                    SeriesOwned::<series_api::ReadingOwned>::from_ww_bytes_owned(&self.log)
                        .unwrap()
                        .0;
                SetResult::Set
            }

            fn get_log(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Series<'_, series_api::Reading<'_>>, ()> {
                GetResult::Value(Series::from_ww_bytes(&self.log).unwrap())
            }

            fn samples(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
                Ready(())
            }

            fn close_samples(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn readings(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                n: u8,
            ) -> RpcResult<Series<'_, series_api::Reading<'_>>> {
                self.readings = super::readings(n as usize);
                Ready(Series::new(&self.readings))
            }

            fn history(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Series<'_, ww_si::Timed<ww_si::Volt>>> {
                self.history = super::history();
                Ready(Series::new(&self.history))
            }

            fn batches(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
                Ready(())
            }

            fn close_batches(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn sideband_batches(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }

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
                series_api :: Sequences for super::Server,
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
                series_api :: Sequences for super::StdClient,
                client = "std_client",
                // debug_to_file = "../../target/tests_series_client.rs"
            );
        }
    }

    use server::api_impl;
    use std_client::StdClient;

    /// Readings that switch scale, status and label now and then.
    fn readings(n: usize) -> Vec<series_api::Reading<'static>> {
        use series_api::{Reading, Scale, Status};
        const NOTES: [&str; 2] = ["calibrated", "warm"];
        (0..n)
            .map(|k| Reading {
                scale: if k % 7 < 4 {
                    Scale::Milli
                } else {
                    Scale::Micro
                },
                value: 3300 + (k as i32 % 5) - 2,
                label: ["a", "a", "b"][k % 3],
                notes: RefVec::Slice {
                    slice: &NOTES[..k % 3],
                },
                status: match k % 5 {
                    0 => Status::Idle,
                    1 | 2 => Status::Busy(k as u8),
                    _ => Status::Fault { code: 7 },
                },
                ok: (k % 4 != 0).then_some(k % 2 == 0),
            })
            .collect()
    }

    fn history() -> Vec<ww_si::Timed<ww_si::Volt>> {
        use ww_si::{Prefix, Second, Timed, Volt};
        (0..50u32)
            .map(|k| Timed {
                t: Second::from_parts(Prefix::Micro, NumericValue::U32(1_000_000 + k * 1000)),
                value: Volt::from_parts(Prefix::Milli, NumericValue::I32(3300 + (k as i32 % 3))),
            })
            .collect()
    }

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
            readings: vec![],
            history: vec![],
            log: Series::<series_api::Reading>::new(&[])
                .to_ww_bytes_owned()
                .unwrap(),
        };
        let device = tests_common::start_device(
            path,
            server,
            series_api::SEQUENCES_FULL_GID,
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
        let mut bundle = wire_weaver_core::load(&path, Some("Sequences".into()), false).unwrap();
        // generic instances (`Timed<Volt>`) replaced with their own definitions, as for a device's bundle
        wire_weaver_client::ww_self::generics::prepare(&mut bundle).unwrap();
        Arc::new(bundle)
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

    /// `items` as a plain `Vec<T>` read by the dynamic decoder: what a `Series` of them must read as.
    fn plain_dyn<T: SerializeShrinkWrapOwned + Clone>(
        items: &[T],
        item_ty: &TypeOwned,
        bundle: &ApiBundleOwned,
    ) -> ValueOwned {
        let bytes = items.to_vec().to_ww_bytes_owned().unwrap();
        ValueOwned::des_shrink_wrap_dyn(&bytes, &TypeOwned::Vec(Box::new(item_ty.clone())), bundle)
            .unwrap()
    }

    fn return_ty(bundle: &ApiBundleOwned, id: u32) -> TypeOwned {
        let ApiItemKindOwned::Method { return_ty, .. } = &bundle.get_item(&[id]).unwrap().kind
        else {
            panic!("method");
        };
        return_ty.clone().unwrap()
    }

    fn owned_readings(n: usize) -> Vec<series_api::ReadingOwned> {
        readings(n)
            .into_iter()
            .map(|r| series_api::ReadingOwned {
                scale: r.scale,
                value: r.value,
                label: r.label.into(),
                notes: r.notes.iter().map(|s| s.unwrap().to_string()).collect(),
                status: r.status,
                ok: r.ok,
            })
            .collect()
    }

    #[test]
    fn series_in_introspection() {
        let bundle = bundle();
        let readings = return_ty(&bundle, 6);
        let TypeOwned::Series(item) = &readings else {
            panic!("{readings:?}");
        };
        assert_eq!(
            readings.human_name(false, &bundle).unwrap(),
            "Series<Reading>"
        );
        let TypeOwned::Struct(reading) = item.get_in_line(&bundle).unwrap() else {
            panic!("struct");
        };
        let opaque: Vec<_> = match &reading.fields {
            wire_weaver_client::ww_self::FieldsOwned::Named(fields) => fields
                .iter()
                .filter(|f| f.series_opaque)
                .map(|f| f.ident.clone().unwrap())
                .collect(),
            _ => panic!("named"),
        };
        assert_eq!(opaque, ["notes"]);
        let history = return_ty(&bundle, 7);
        assert!(matches!(history, TypeOwned::Series(_)), "{history:?}");
    }

    /// The bytes of the generated server's `Series`, read by the dynamic decoder, are the items the typed client
    /// reads.
    #[test]
    fn dynamic_decode_equals_typed() {
        let bundle = bundle();
        let ty = return_ty(&bundle, 6);
        let TypeOwned::Series(item_ty) = &ty else {
            panic!("series");
        };
        for n in [0, 1, 2, 9, 100] {
            let items = readings(n);
            let bytes = Series::new(&items).to_ww_bytes_owned().unwrap();
            let typed =
                SeriesOwned::<series_api::ReadingOwned>::from_ww_bytes_owned(&bytes).unwrap();
            assert_eq!(typed.0, owned_readings(n));
            let dynamic = ValueOwned::des_shrink_wrap_dyn(&bytes, &ty, &bundle).unwrap();
            assert_eq!(dynamic, plain_dyn(&typed.0, item_ty, &bundle), "{n} items");
            let columns = ValueOwned::des_series_columns_dyn(&bytes, &ty, &bundle).unwrap();
            assert_eq!(columns.len, n);
        }

        let ty = return_ty(&bundle, 7);
        let TypeOwned::Series(item_ty) = &ty else {
            panic!("series");
        };
        let items = history();
        let bytes = Series::new(&items).to_ww_bytes_owned().unwrap();
        assert!(bytes.len() < items.len() * 2, "{} bytes", bytes.len());
        let dynamic = ValueOwned::des_shrink_wrap_dyn(&bytes, &ty, &bundle).unwrap();
        assert_eq!(dynamic, plain_dyn(&items, item_ty, &bundle));
    }

    /// `readings(6)` as the generated server writes it, also decoded into numpy columns by
    /// `wire_weaver_py/tests/test_offline.py`.
    const READINGS_6_HEX: &str = "6609884dce2671b3919cace6671040618880628080618880620800860863616c696272617465640291841063616c696272617465647761726d4292808000860863616c696272617465640291841063616c696272617465647761726d429204062510093141c212810800";

    #[test]
    fn readings_golden_bytes() {
        let bytes = Series::new(&readings(6)).to_ww_bytes_owned().unwrap();
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, READINGS_6_HEX);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn series_through_clients_and_streams() {
        let (device, _data) = start("series/series");
        let mut client = connect(&device).await;
        let readings = client.readings(40).call().await.unwrap();
        assert_eq!(readings.0, owned_readings(40));
        let history = client.history().call().await.unwrap();
        assert_eq!(history.0, history_items());

        let mut batches = client.batches().call().await.unwrap();
        let items = super::tests::readings(12);
        let mut scratch = [0u8; 512];
        device.send(
            api_impl::stream_data_ser()
                .batches(&Series::new(&items), &mut scratch)
                .unwrap(),
        );
        assert_eq!(batches.recv().await.unwrap().0, owned_readings(12));
        drop(batches);
        drop(client);

        let (device, data) = start("series/series-dynamic");
        let dynamic = DynClient::from_config(ClientConfig::new().in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");
        let bundle = bundle();
        let root = DynResource::root(bundle.clone());
        let r = root
            .child("readings")
            .unwrap()
            .call(dynamic.cmd(), &[ValueOwned::Numeric(NumericValue::U8(9))])
            .await
            .unwrap()
            .unwrap();
        let TypeOwned::Series(item_ty) = return_ty(&bundle, 6) else {
            panic!("series");
        };
        assert_eq!(r, plain_dyn(&owned_readings(9), &item_ty, &bundle));

        // a dynamic client writes a Series (CLT-29), the generated server reads the items it sent
        let log = root.child("log").unwrap();
        let values = plain_dyn(&owned_readings(30), &item_ty, &bundle);
        log.write(dynamic.cmd(), &values).await.unwrap();
        assert_eq!(data.lock().unwrap().log, owned_readings(30));
        assert_eq!(log.read(dynamic.cmd()).await.unwrap(), values);
    }

    /// The dynamic encoder writes the bytes the generated code writes for the same items and policy (CLT-29).
    #[test]
    fn dynamic_encode_equals_typed() {
        use wire_weaver::shrink_wrap::series::{CodecPolicy, DeltaPacked};
        let bundle = bundle();
        let ty = return_ty(&bundle, 6);
        let TypeOwned::Series(item_ty) = &ty else {
            panic!("series");
        };
        for n in [0, 1, 2, 9, 100] {
            let items = owned_readings(n);
            let values = plain_dyn(&items, item_ty, &bundle);
            for policy in [CodecPolicy::SMALLEST, DeltaPacked.into()] {
                let typed = SeriesOwned(items.clone())
                    .as_borrowed_with(policy)
                    .to_ww_bytes_owned()
                    .unwrap();
                let dynamic = values.ser_series_dyn(&ty, &bundle, policy).unwrap();
                assert_eq!(dynamic, typed, "{n} items, {policy:?}");
            }
            assert_eq!(
                values.ser_shrink_wrap_dyn(&ty, &bundle).unwrap(),
                Series::new(&readings(n)).to_ww_bytes_owned().unwrap()
            );
        }

        let ty = return_ty(&bundle, 7);
        let TypeOwned::Series(item_ty) = &ty else {
            panic!("series");
        };
        let items = history();
        let values = plain_dyn(&items, item_ty, &bundle);
        for policy in [CodecPolicy::SMALLEST, DeltaPacked.into()] {
            let typed = Series::with_policy(&items, policy)
                .to_ww_bytes_owned()
                .unwrap();
            assert_eq!(values.ser_series_dyn(&ty, &bundle, policy).unwrap(), typed);
        }
    }

    fn history_items() -> Vec<ww_si::Timed<ww_si::Volt>> {
        history()
    }
}

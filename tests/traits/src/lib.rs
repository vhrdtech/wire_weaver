#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};
    use wire_weaver::prelude::*;

    #[derive(Default)]
    struct SharedTestData {
        subgroup_m1_called: bool,
        gpio_used_indices: Vec<u32>,
        set_gain: HashMap<[UNib32; 2], f32>,
    }

    mod no_std_sync_server {
        use super::*;
        use std::sync::{Arc, RwLock};

        pub struct NoStdSyncServer {
            pub data: Arc<RwLock<SharedTestData>>,
        }

        impl NoStdSyncServer {
            fn g1_m1(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
                self.data.write().unwrap().subgroup_m1_called = true;
                Ready(())
            }

            fn gpio_set_high(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index: [UNib32; 1],
            ) -> RpcResult<()> {
                self.data
                    .write()
                    .unwrap()
                    .gpio_used_indices
                    .push(index[0].0);
                Ready(())
            }

            fn set_periph_channel_gain(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index: [UNib32; 2],
                gain: f32,
            ) -> SetResult<()> {
                self.data.write().unwrap().set_gain.insert(index, gain);
                Set
            }

            fn get_periph_channel_gain(
                &self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index: [UNib32; 2],
            ) -> GetResult<f32, ()> {
                let value = self
                    .data
                    .read()
                    .unwrap()
                    .set_gain
                    .get(&index)
                    .copied()
                    .unwrap_or(0.0);
                Value(value)
            }

            fn periph_channel_run(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _index: [UNib32; 2],
            ) -> RpcResult<()> {
                Ready(())
            }

            fn i2c_i2c_write(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                data: u8,
            ) -> RpcResult<u8> {
                Ready(data + 1)
            }

            fn valid_indices_root_gpio(&mut self) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..255)
            }

            fn valid_indices_root_periph(&mut self) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..255)
            }

            fn valid_indices_root_periph_channel(
                &mut self,
                _index: [UNib32; 1],
            ) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..255)
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                traits_api :: Traits for super::NoStdSyncServer,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                introspect = "no_docs",
                // optional items, no handlers for them above
                omit = ["reset", "i2c.i2c_wiggle_clock", "i2c.pullups", "i2c.i2c_monitor"],
                // debug_to_file = "../../target/tests_traits_server.rs"
            );
        }

        impl tests_common::TestProcessEvents for NoStdSyncServer {
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

    /// Optional items left out under `omit_if` conditions: `all()` is always true, `any()` never.
    mod cfg_server {
        use super::*;

        pub struct CfgServer;

        impl CfgServer {
            fn g1_m1(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
                Ready(())
            }

            fn gpio_set_high(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _index: [UNib32; 1],
            ) -> RpcResult<()> {
                Ready(())
            }

            fn set_periph_channel_gain(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _index: [UNib32; 2],
                _gain: f32,
            ) -> SetResult<()> {
                Set
            }

            fn get_periph_channel_gain(
                &self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _index: [UNib32; 2],
            ) -> GetResult<f32, ()> {
                Value(0.0)
            }

            fn periph_channel_run(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _index: [UNib32; 2],
            ) -> RpcResult<()> {
                Ready(())
            }

            fn i2c_i2c_write(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                data: u8,
            ) -> RpcResult<u8> {
                Ready(data + 1)
            }

            // not omitted: its omit_if condition is false
            fn i2c_i2c_wiggle_clock(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                count: u8,
            ) -> RpcResult<u8> {
                Ready(count * 2)
            }

            fn valid_indices_root_gpio(&mut self) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..255)
            }

            fn valid_indices_root_periph(&mut self) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..255)
            }

            fn valid_indices_root_periph_channel(
                &mut self,
                _index: [UNib32; 1],
            ) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..255)
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                traits_api :: Traits for super::CfgServer,
                server = true, no_alloc = true, use_async = false,
                introspect = "no_docs",
                omit = ["reset", "i2c.i2c_monitor"],
                omit_if(all()) = ["i2c.pullups"],
                omit_if(any()) = ["i2c.i2c_wiggle_clock"],
            );
        }

        impl tests_common::TestProcessEvents for CfgServer {
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
                traits_api :: Traits for super::StdClient,
                client = "std_client",
                // debug_to_file = "../../target/tests_traits_client.rs"
            );
        }
    }

    // mod no_std_raw_client {
    //     use super::*;
    //
    //     pub struct RawClient {}
    //
    //     ww_api!(
    //         "properties.rs" as tests::Properties for RawClient,
    //         client = "raw",
    //         no_alloc = true,
    //         use_async = false,
    //     );
    // }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn std_async_client_driving_no_std_sync_server() {
        let _ = tracing_subscriber::fmt::try_init();
        let data = Arc::new(RwLock::new(SharedTestData::default()));
        let server = no_std_sync_server::NoStdSyncServer { data: data.clone() };
        let device = tests_common::start_device(
            "traits",
            server,
            traits_api::TRAITS_FULL_GID,
            no_std_sync_server::api_impl::api_hash(),
        );
        let client = std_client::StdClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");

        client.g1().m1().call().await.unwrap();
        assert!(data.read().unwrap().subgroup_m1_called);

        client.gpio(0).set_high().call().await.unwrap();
        assert!(data.read().unwrap().gpio_used_indices.contains(&0));

        client.gpio(123).set_high().call().await.unwrap();
        assert!(data.read().unwrap().gpio_used_indices.contains(&123));

        client
            .periph(3)
            .channel(7)
            .write_gain(10.0)
            .write()
            .await
            .unwrap();
        assert_eq!(
            data.read().unwrap().set_gain.get(&[UNib32(3), UNib32(7)]),
            Some(&10.0)
        );
        let value = client
            .periph(3)
            .channel(7)
            .read_gain()
            .read()
            .await
            .unwrap();
        assert!(value == 10.0);
    }

    fn assert_not_implemented<T: std::fmt::Debug>(
        result: Result<T, wire_weaver_client::Error>,
        name: &str,
    ) {
        match result {
            Err(wire_weaver_client::Error::NotImplementedByDevice { resource, .. }) => {
                assert_eq!(resource, name)
            }
            other => panic!("{name}: expected NotImplementedByDevice, got {other:?}"),
        }
    }

    fn assert_call_unimplemented<T: std::fmt::Debug, E: std::fmt::Debug>(
        result: Result<T, wire_weaver_client::CallError<E>>,
        name: &str,
    ) {
        match result {
            Err(wire_weaver_client::CallError::Unimplemented { resource }) => {
                assert_eq!(resource, name)
            }
            other => panic!("{name}: expected CallError::Unimplemented, got {other:?}"),
        }
    }

    /// Omitted optional items fail locally: the device's introspection data lists them.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn omitted_items_fail_locally() {
        let _ = tracing_subscriber::fmt::try_init();
        let data = Arc::new(RwLock::new(SharedTestData::default()));
        let server = no_std_sync_server::NoStdSyncServer { data };
        let device = tests_common::start_device(
            "traits/omitted",
            server,
            traits_api::TRAITS_FULL_GID,
            no_std_sync_server::api_impl::api_hash(),
        );
        let client = std_client::StdClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");

        assert_eq!(client.i2c().i2c_write(1).call().await.unwrap(), 2);
        // method
        assert_not_implemented(
            client.i2c().i2c_wiggle_clock(3).call().await,
            "i2c.i2c_wiggle_clock",
        );
        // group: a method and a property
        // with an error type: the CallError variant (CLT-24), not the method's own error
        assert_call_unimplemented(
            client.i2c().i2c_set_pullups(true).call().await,
            "i2c.i2c_set_pullups",
        );
        assert_not_implemented(
            client.i2c().write_i2c_pullups(true).write().await,
            "i2c.i2c_pullups",
        );
        assert_not_implemented(
            client.i2c().read_i2c_pullups().read().await,
            "i2c.i2c_pullups",
        );
        // an item of the root level
        assert_not_implemented(client.reset().call().await, "reset");
    }

    /// `omit_if(all())` leaves the group out, `omit_if(any())` keeps the method, in dispatch and introspection.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn omit_if_follows_cfg() {
        let _ = tracing_subscriber::fmt::try_init();
        let device = tests_common::start_device(
            "traits/omit_if",
            cfg_server::CfgServer,
            traits_api::TRAITS_FULL_GID,
            cfg_server::api_impl::api_hash(),
        );
        let client = std_client::StdClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");

        assert_eq!(client.i2c().i2c_wiggle_clock(3).call().await.unwrap(), 6);
        assert_call_unimplemented(
            client.i2c().i2c_set_pullups(true).call().await,
            "i2c.i2c_set_pullups",
        );
        assert_not_implemented(
            client.i2c().write_i2c_pullups(true).write().await,
            "i2c.i2c_pullups",
        );
    }

    /// Error kind a server answers a request to `path` with, `None` if it succeeds.
    fn call_raw(
        process: impl FnOnce(&[u8], &mut [u8], &mut wire_weaver::EventQueue) -> Vec<u8>,
        path: &[u32],
        args: &[u8],
    ) -> Option<String> {
        use ww_client_server::{Event, PathKind, Request, RequestKind};
        let path: Vec<UNib32> = path.iter().map(|id| UNib32(*id)).collect();
        let request = Request {
            seq: wire_weaver::shrink_wrap::UVlq32Backfill(1),
            path_kind: PathKind::Absolute {
                path: RefVec::Slice { slice: &path },
            },
            kind: RequestKind::Call {
                args: wire_weaver::shrink_wrap::tail_bytes::TailBytes(args),
            },
        };
        let mut request_buf = [0u8; 64];
        let request = request.to_ww_bytes(&mut request_buf).unwrap();
        let mut scratch = [0u8; 256];
        let mut queue_buf = [0u8; 256];
        let mut out = wire_weaver::EventQueue::new(&mut queue_buf);
        let event = process(request, &mut scratch, &mut out);
        let event = Event::from_ww_bytes(&event).unwrap();
        event.result.err().map(|e| format!("{:?}", e.kind))
    }

    /// Called anyway (a client that doesn't know the device's model), an omitted item answers `Unimplemented`, an
    /// id past the trait's items `BadPath`.
    #[test]
    fn omitted_items_answer_unimplemented() {
        let data = Arc::new(RwLock::new(SharedTestData::default()));
        let mut server = no_std_sync_server::NoStdSyncServer { data };
        let mut call = |path: &[u32], args: &[u8]| {
            call_raw(
                |request, scratch, out| {
                    server
                        .process_request_bytes(request, scratch, out, ())
                        .unwrap()
                        .to_vec()
                },
                path,
                args,
            )
        };
        // ids: root g1 0, gpio 1, periph 2, i2c 3, reset 4; i2c: write 0, wiggle 1, set_pullups 2, pullups 3,
        // monitor 4
        assert_eq!(call(&[3, 0], &[1]), None);
        assert_eq!(call(&[3, 1], &[3]).as_deref(), Some("Unimplemented"));
        assert_eq!(call(&[3, 2], &[0x80]).as_deref(), Some("Unimplemented"));
        assert_eq!(call(&[3, 4], &[]).as_deref(), Some("Unimplemented"));
        assert_eq!(call(&[3, 5], &[]).as_deref(), Some("BadPath"));
        assert_eq!(call(&[4], &[]).as_deref(), Some("Unimplemented"));
        assert_eq!(call(&[5], &[]).as_deref(), Some("BadPath"));

        let mut server = cfg_server::CfgServer;
        let mut call = |path: &[u32], args: &[u8]| {
            call_raw(
                |request, scratch, out| {
                    server
                        .process_request_bytes(request, scratch, out, ())
                        .unwrap()
                        .to_vec()
                },
                path,
                args,
            )
        };
        assert_eq!(call(&[3, 1], &[3]), None);
        assert_eq!(call(&[3, 2], &[0x80]).as_deref(), Some("Unimplemented"));
    }

    #[test]
    fn scaffold_leaves_out_omitted_items() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../traits_api");
        let bundle = wire_weaver_core::load(&path, Some("Traits".into()), false).unwrap();
        let config = wire_weaver_core::ServerScaffoldConfig {
            server_struct: "Server".into(),
            no_alloc: true,
            use_async: false,
            method_model: String::new(),
            property_model: String::new(),
            const_model: String::new(),
            medium: None,
            omit: vec!["reset".into(), "i2c.pullups".into()],
            observe_cells: vec![],
        };
        let code = wire_weaver_core::gen_server_scaffold(&bundle, &config).unwrap();
        for expected in [
            "fn i2c_i2c_write(",
            "fn i2c_i2c_wiggle_clock(",
            "// optional, can be left out with omit = [\"i2c.i2c_wiggle_clock\"]",
            "omit = [\"reset\", \"i2c.pullups\"],",
        ] {
            assert!(code.contains(expected), "{expected}\n{code}");
        }
        for omitted in ["fn reset(", "i2c_set_pullups", "i2c_pullups("] {
            assert!(!code.contains(omitted), "{omitted}\n{code}");
        }

        let config = wire_weaver_core::ServerScaffoldConfig {
            omit: vec!["i2c.i2c_write".into()],
            observe_cells: vec![],
            ..config
        };
        let e = wire_weaver_core::gen_server_scaffold(&bundle, &config).unwrap_err();
        assert!(e.to_string().contains("not #[optional]"), "{e}");
    }

    /// The dynamic client (used by Python) knows omitted items from the device's model and fails them locally.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dynamic_client_fails_omitted_items_locally() {
        use wire_weaver_client::{ClientConfig, DynClient, DynResource};
        let _ = tracing_subscriber::fmt::try_init();
        let data = Arc::new(RwLock::new(SharedTestData::default()));
        let device = tests_common::start_device(
            "traits/dynamic",
            no_std_sync_server::NoStdSyncServer { data },
            traits_api::TRAITS_FULL_GID,
            no_std_sync_server::api_impl::api_hash(),
        );
        let client = DynClient::from_config(ClientConfig::new().in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");
        let bundle = client.device_introspect().unwrap().api_bundle.clone();
        let root = DynResource::root(bundle);
        let i2c = root.child("i2c").unwrap();
        assert!(!i2c.child("i2c_write").unwrap().is_omitted());
        let wiggle = i2c.child("i2c_wiggle_clock").unwrap();
        assert!(wiggle.is_omitted());
        assert!(root.child("reset").unwrap().is_omitted());
        let r = wiggle
            .call(
                client.cmd(),
                &[wire_weaver_client::ww_self::ValueOwned::Numeric(
                    wire_weaver_client::ww_numeric::NumericValue::U8(3),
                )],
            )
            .await;
        assert_not_implemented(r, "i2c.i2c_wiggle_clock");
        assert_not_implemented(
            i2c.child("i2c_pullups").unwrap().read(client.cmd()).await,
            "i2c.i2c_pullups",
        );
    }
}

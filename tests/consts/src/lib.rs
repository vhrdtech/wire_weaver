//! `const` items (API-36): served from the server's consts impl without handlers, as a Rust const or a `fn`, and
//! read by clients once per connection. A mock server on `ww_device::Server` through `tests_common::start_device`,
//! clients through the real host event loop.

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver_client::{ClientConfig, DynClient, DynResource};
    use ww_client_server::{Request, RequestKind};

    mod server {
        use super::*;
        use consts_api::Limits;
        use tests_common::TestProcessEvents;
        use wire_weaver::BlockingEventOut;

        pub struct Server {
            /// Read from the chip at boot in a real firmware
            pub serial: String,
            pub gains: [u8; 2],
            pub uptime: u32,
        }

        impl Server {
            fn get_uptime(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<u32, ()> {
                self.uptime += 1;
                Value(self.uptime)
            }

            fn valid_indices_root_channel(&mut self) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..2)
            }
        }

        /// No handlers for consts: Rust consts, and `fn`s for `SERIAL` and `GAIN` (`const_model`).
        impl api_impl::ConstsTestConsts for Server {
            const LIMITS: Limits<'static> = Limits {
                max_len: 64,
                names: RefVec::Slice { slice: &["a", "b"] },
            };

            fn serial(&self) -> &str {
                &self.serial
            }

            const EXTRA: u8 = 7;

            const INFO_NAME: &'static str = "info";

            fn channel_gain(&self, index_chain: [UNib32; 1]) -> u8 {
                self.gains[index_chain[0].0 as usize]
            }

            const CHANNEL_KIND: &'static str = "adc";
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                consts_api :: ConstsTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                const_model = "SERIAL|GAIN=fn",
                introspect = "no_docs",
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

    /// Async, and leaves out the optional `EXTRA`: no item for it in the consts trait.
    mod async_server {
        use super::*;
        use consts_api::Limits;
        use tests_common::TestProcessEventsAsync;
        use wire_weaver::EventOut;

        pub struct Server;

        impl Server {
            async fn get_uptime(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
            ) -> GetResult<u32, ()> {
                Value(0)
            }

            fn valid_indices_root_channel(&mut self) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..1)
            }
        }

        impl api_impl::ConstsTestConsts for Server {
            const LIMITS: Limits<'static> = Limits {
                max_len: 8,
                names: RefVec::Slice { slice: &[] },
            };
            const SERIAL: &'static str = "async";
            const INFO_NAME: &'static str = "async info";
            const CHANNEL_GAIN: u8 = 1;
            const CHANNEL_KIND: &'static str = "dac";
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                consts_api :: ConstsTest for super::Server,
                server = true, no_alloc = true, use_async = true,
                method_model = "_=immediate",
                property_model = "_=get_set",
                introspect = "no_docs",
                omit = ["EXTRA"],
            );
        }

        impl TestProcessEventsAsync for Server {
            type Medium = ();

            async fn process_request_bytes<'a>(
                &mut self,
                bytes: &[u8],
                scratch: &'a mut [u8],
                out: &mut impl EventOut,
                medium: (),
            ) -> Result<&'a [u8], ShrinkWrapError> {
                self.process_request_bytes(bytes, scratch, out, medium)
                    .await
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
                consts_api :: ConstsTest for super::StdClient,
                client = "std_client",
            );
        }
    }

    use std_client::StdClient;

    /// The device, and how many read requests reached it since the start.
    fn start(path: &str) -> (TestDevice, Arc<AtomicU32>) {
        let _ = tracing_subscriber::fmt::try_init();
        let device = tests_common::start_device(
            path,
            server::Server {
                serial: "E661A410".into(),
                gains: [3, 5],
                uptime: 0,
            },
            consts_api::CONSTS_TEST_FULL_GID,
            server::api_impl::api_hash(),
        );
        let reads = Arc::new(AtomicU32::new(0));
        let counter = reads.clone();
        device.filter_requests(move |bytes| {
            if let Ok(Request {
                kind: RequestKind::Read,
                ..
            }) = Request::from_ww_bytes(bytes)
            {
                counter.fetch_add(1, Ordering::Relaxed);
            }
            false
        });
        (device, reads)
    }

    async fn connect(device: &TestDevice) -> StdClient {
        StdClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn values_of_both_forms() {
        let (device, _) = start("consts/values");
        let client = connect(&device).await;
        let limits = client.read_LIMITS().read().await.unwrap();
        assert_eq!(limits.max_len, 64);
        assert_eq!(limits.names, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(client.read_SERIAL().read().await.unwrap(), "E661A410");
        assert_eq!(client.read_EXTRA().read().await.unwrap(), 7);
        assert_eq!(client.info().read_NAME().read().await.unwrap(), "info");
        assert_eq!(client.channel(0).read_GAIN().read().await.unwrap(), 3);
        assert_eq!(client.channel(1).read_GAIN().read().await.unwrap(), 5);
        assert_eq!(client.channel(1).read_KIND().read().await.unwrap(), "adc");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn second_read_makes_no_request() {
        let (device, reads) = start("consts/cached");
        let client = connect(&device).await;
        let before = reads.load(Ordering::Relaxed);

        assert_eq!(client.read_SERIAL().read().await.unwrap(), "E661A410");
        assert_eq!(reads.load(Ordering::Relaxed), before + 1);
        assert_eq!(client.read_SERIAL().read().await.unwrap(), "E661A410");
        assert_eq!(reads.load(Ordering::Relaxed), before + 1);

        // each const and each index of an array is its own entry
        assert_eq!(client.channel(0).read_GAIN().read().await.unwrap(), 3);
        assert_eq!(client.channel(1).read_GAIN().read().await.unwrap(), 5);
        assert_eq!(client.channel(0).read_GAIN().read().await.unwrap(), 3);
        assert_eq!(client.channel(1).read_GAIN().read().await.unwrap(), 5);
        assert_eq!(reads.load(Ordering::Relaxed), before + 3);

        // properties are read every time
        assert_eq!(client.read_uptime().read().await.unwrap(), 1);
        assert_eq!(client.read_uptime().read().await.unwrap(), 2);
        assert_eq!(reads.load(Ordering::Relaxed), before + 5);

        // a cleared cache, as after a disconnect, asks again
        client.cmd.clear_const_cache();
        assert_eq!(client.read_SERIAL().read().await.unwrap(), "E661A410");
        assert_eq!(reads.load(Ordering::Relaxed), before + 6);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn each_connection_reads_once() {
        let (device, reads) = start("consts/connections");
        let client = connect(&device).await;
        client.read_LIMITS().read().await.unwrap();
        client.read_LIMITS().read().await.unwrap();
        let after_first = reads.load(Ordering::Relaxed);
        client.cmd.disconnect().asynch().await.unwrap();

        let client = connect(&device).await;
        let before = reads.load(Ordering::Relaxed);
        assert!(before >= after_first);
        client.read_LIMITS().read().await.unwrap();
        client.read_LIMITS().read().await.unwrap();
        assert_eq!(reads.load(Ordering::Relaxed), before + 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn async_server_without_optional_const() {
        let _ = tracing_subscriber::fmt::try_init();
        let device = tests_common::start_device_async(
            "consts/async",
            async_server::Server,
            consts_api::CONSTS_TEST_FULL_GID,
            async_server::api_impl::api_hash(),
        );
        let client = connect(&device).await;
        assert_eq!(client.read_SERIAL().read().await.unwrap(), "async");
        assert_eq!(
            client.info().read_NAME().read().await.unwrap(),
            "async info"
        );
        assert_eq!(client.channel(0).read_GAIN().read().await.unwrap(), 1);
        assert_eq!(client.read_LIMITS().read().await.unwrap().max_len, 8);
        assert!(client.read_EXTRA().read().await.is_err());
    }

    /// The dynamic client (used by Python and `ww introspect`) caches consts too.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dynamic_client_reads_once() {
        let (device, reads) = start("consts/dynamic");
        let client = DynClient::from_config(ClientConfig::new().in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");
        let root = DynResource::root(client.device_introspect().unwrap().api_bundle.clone());
        let before = reads.load(Ordering::Relaxed);
        let name = root.child("info").unwrap().child("NAME").unwrap();
        for _ in 0..3 {
            let value = name.read(client.cmd()).await.unwrap();
            assert_eq!(
                value,
                wire_weaver_client::ww_self::ValueOwned::String("info".into())
            );
        }
        let gain = root.child("channel").unwrap().index(1).unwrap();
        let gain = gain.child("GAIN").unwrap();
        gain.read(client.cmd()).await.unwrap();
        gain.read(client.cmd()).await.unwrap();
        assert_eq!(reads.load(Ordering::Relaxed), before + 2);

        let uptime = root.child("uptime").unwrap();
        uptime.read(client.cmd()).await.unwrap();
        uptime.read(client.cmd()).await.unwrap();
        assert_eq!(reads.load(Ordering::Relaxed), before + 4);
    }

    /// A promise fills the cache and is answered from it.
    #[test]
    fn promise_reads_once() {
        let (device, reads) = start("consts/promise");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let client = StdClient::config(|c| c.in_process_path(device.path()))
            .connect_blocking()
            .expect("connect");
        let before = reads.load(Ordering::Relaxed);
        for _ in 0..2 {
            let mut read = client.info().read_NAME().read_promise("name");
            tests_common::wait_promise(&mut read);
            assert_eq!(read.take_ready().as_deref(), Some("info"));
        }
        assert_eq!(client.info().read_NAME().blocking_read().unwrap(), "info");
        assert_eq!(reads.load(Ordering::Relaxed), before + 1);
    }

    #[test]
    fn scaffold_has_consts_impl() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../consts_api");
        let bundle = wire_weaver_core::load_for_codegen(&path, Some("ConstsTest".into())).unwrap();
        let config = wire_weaver_core::ServerScaffoldConfig {
            server_struct: "Server".into(),
            no_alloc: true,
            use_async: false,
            method_model: String::new(),
            property_model: String::new(),
            const_model: "SERIAL|GAIN=fn".into(),
            medium: None,
            omit: vec![],
        };
        let code = wire_weaver_core::gen_server_scaffold(&bundle, &config).unwrap();
        let code: String = code.split_whitespace().collect::<Vec<_>>().join(" ");
        for expected in [
            "impl api_impl::ConstsTestConsts for Server {",
            "const LIMITS: consts_api :: Limits < 'static > = todo!();",
            "fn serial(&self, ) -> & '_ str { todo!() }",
            "// optional, can be left out with omit = [\"EXTRA\"]",
            "const INFO_NAME: & 'static str = todo!();",
            "fn channel_gain(&self, _index_chain: [UNib32; 1], ) -> u8 { todo!() }",
            "const_model = \"SERIAL|GAIN=fn\",",
        ] {
            assert!(code.contains(expected), "{expected}\n{code}");
        }
        assert!(!code.contains("get_LIMITS"), "{code}");
    }
}

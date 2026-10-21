#[cfg(test)]
mod tests {
    use properties_api::{CustomOwned, InnerOwned, ModeError};
    use std::sync::{Arc, RwLock};
    use std::time::Duration;
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver_client::{CallError, Error, MultiRead, MultiWrite};
    use ww_client_server::ErrorKindOwned;

    /// Elements of the `ch[]` array (CLT-15 multi-request tests)
    const CHANNELS: u32 = 4;

    #[derive(Default)]
    struct SharedTestData {
        x: u8,
        y_changed: u8,
        custom: CustomOwned,
        mode: u8,
        ch_level: [u8; CHANNELS as usize],
    }

    /// Same handlers for all servers, they only differ in `multi_req` and `no_alloc`: `alloc` for the
    /// `no_alloc = false` ones, which take and give `custom` owned (API-21)
    macro_rules! handlers {
        () => {
            common_handlers!();

            fn get_custom(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Custom<'_>, ()> {
                Value(Custom {
                    z: 123,
                    inner: RefVec::Slice {
                        slice: &[Inner { u: 63, v: "abc" }, Inner { u: 127, v: "def" }],
                    },
                })
            }

            fn set_custom(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                custom: Custom<'_>,
            ) -> SetResult<()> {
                let custom = custom.make_owned();
                self.data.write().unwrap().custom = custom;
                Set
            }
        };
        (alloc) => {
            common_handlers!();

            fn get_custom(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<CustomOwned, ()> {
                Value(CustomOwned {
                    z: 123,
                    inner: vec![
                        InnerOwned {
                            u: 63,
                            v: "abc".into(),
                        },
                        InnerOwned {
                            u: 127,
                            v: "def".into(),
                        },
                    ],
                })
            }

            fn set_custom(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                custom: CustomOwned,
            ) -> SetResult<()> {
                self.data.write().unwrap().custom = custom;
                Set
            }
        };
    }

    macro_rules! common_handlers {
        () => {
            fn set_x(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: u8,
            ) -> SetResult<()> {
                self.data.write().unwrap().x = value;
                // notify observers, through the handler's context
                api_impl::stream_data_ser()
                    .x_send_blocking(&value, _cx)
                    .unwrap();
                Set
            }

            fn get_x(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> GetResult<u8, ()> {
                Value(self.data.read().unwrap().x)
            }

            fn changed_y(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {
                self.data.write().unwrap().y_changed += 1;
                api_impl::stream_data_ser()
                    .y_send_blocking(&self.y, _cx)
                    .unwrap();
            }

            fn set_absent(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _value: u8,
            ) -> SetResult<()> {
                Unimplemented.into()
            }

            fn get_absent(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<u8, ()> {
                Unimplemented.into()
            }

            fn set_mode(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: u8,
            ) -> SetResult<ModeError> {
                if value == 99 {
                    return SetError(ModeError::Locked);
                }
                if value > 10 {
                    return SetError(ModeError::TooBig);
                }
                self.data.write().unwrap().mode = value;
                Set
            }

            fn get_mode(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<u8, ModeError> {
                match self.data.read().unwrap().mode {
                    0 => GetError(ModeError::NotSet),
                    mode => Value(mode),
                }
            }

            fn valid_indices_root_ch(&mut self) -> ValidIndices<'_> {
                ValidIndices::range_u32(0..CHANNELS)
            }

            fn set_ch_level(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
                value: u8,
            ) -> SetResult<()> {
                self.data.write().unwrap().ch_level[index_chain[0].0 as usize] = value;
                Set
            }

            fn get_ch_level(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index_chain: [UNib32; 1],
            ) -> GetResult<u8, ()> {
                Value(self.data.read().unwrap().ch_level[index_chain[0].0 as usize])
            }
        };
    }

    mod multi_req_server {
        use super::*;
        use properties_api::{Custom, Inner};
        use tests_common::TestProcessEvents;
        use wire_weaver::shrink_wrap::RefVec;
        use wire_weaver::{SetResult, Unimplemented};

        pub struct Server {
            pub data: Arc<RwLock<SharedTestData>>,
            pub y: u8,
        }

        impl Server {
            handlers!();
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                properties_api :: Properties for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "x=get_set, y=value_on_changed",
                multi_req = true
                // debug_to_file = "../target/tests_properties_server.rs" // uncomment if you want to see the resulting AST and generated code
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

    mod single_req_server {
        use super::*;
        use properties_api::{Custom, Inner};
        use tests_common::TestProcessEvents;
        use wire_weaver::shrink_wrap::RefVec;
        use wire_weaver::{SetResult, Unimplemented};

        pub struct Server {
            pub data: Arc<RwLock<SharedTestData>>,
            pub y: u8,
        }

        impl Server {
            handlers!();
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                properties_api :: Properties for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "x=get_set, y=value_on_changed",
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

    mod alloc_multi_req_server {
        use super::*;
        use tests_common::TestProcessEvents;
        use wire_weaver::{SetResult, Unimplemented};

        pub struct Server {
            pub data: Arc<RwLock<SharedTestData>>,
            pub y: u8,
        }

        impl Server {
            handlers!(alloc);
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                properties_api :: Properties for super::Server,
                server = true, no_alloc = false, use_async = false,
                method_model = "_=immediate",
                property_model = "x=get_set, y=value_on_changed",
                multi_req = true
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

    mod alloc_single_req_server {
        use super::*;
        use tests_common::TestProcessEvents;
        use wire_weaver::{SetResult, Unimplemented};

        pub struct Server {
            pub data: Arc<RwLock<SharedTestData>>,
            pub y: u8,
        }

        impl Server {
            handlers!(alloc);
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                properties_api :: Properties for super::Server,
                server = true, no_alloc = false, use_async = false,
                method_model = "_=immediate",
                property_model = "x=get_set, y=value_on_changed",
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
                properties_api :: Properties for super::StdClient,
                client = "std_client",
                // debug_to_file = "../../target/tests_properties_client.rs"
            );
        }
    }

    use std_client::StdClient;

    fn start_multi_req(path: &str) -> (TestDevice, Arc<RwLock<SharedTestData>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let data = Arc::new(RwLock::new(SharedTestData::default()));
        // the alloc server answers, the no_alloc one with its own state must give the same bytes (API-21)
        let server = tests_common::Twin {
            a: alloc_multi_req_server::Server {
                data: data.clone(),
                y: 0xBB,
            },
            b: multi_req_server::Server {
                data: Default::default(),
                y: 0xBB,
            },
        };
        let device = tests_common::start_device(
            path,
            server,
            properties_api::PROPERTIES_FULL_GID,
            alloc_multi_req_server::api_impl::api_hash(),
        );
        (device, data)
    }

    fn start_single_req(path: &str) -> (TestDevice, Arc<RwLock<SharedTestData>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let data = Arc::new(RwLock::new(SharedTestData::default()));
        // the alloc server answers, the no_alloc one with its own state must give the same bytes (API-21)
        let server = tests_common::Twin {
            a: alloc_single_req_server::Server {
                data: data.clone(),
                y: 0xBB,
            },
            b: single_req_server::Server {
                data: Default::default(),
                y: 0xBB,
            },
        };
        let device = tests_common::start_device(
            path,
            server,
            properties_api::PROPERTIES_FULL_GID,
            alloc_single_req_server::api_impl::api_hash(),
        );
        (device, data)
    }

    async fn connect(device: &TestDevice) -> StdClient {
        StdClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    fn connect_blocking(device: &TestDevice) -> StdClient {
        StdClient::config(|c| c.in_process_path(device.path()))
            .connect_blocking()
            .expect("connect")
    }

    fn remote_error_kind(r: Result<impl core::fmt::Debug, Error>) -> ErrorKindOwned {
        match r {
            Err(Error::RemoteError(e)) => e.kind,
            other => panic!("expected RemoteError, got {other:?}"),
        }
    }

    fn user_error(r: Result<impl core::fmt::Debug, Error>) -> ModeError {
        let ErrorKindOwned::UserBytes(bytes) = remote_error_kind(r) else {
            panic!("expected UserBytes");
        };
        ModeError::from_ww_bytes(&bytes).unwrap()
    }

    async fn get_set_and_value_on_changed(client: &StdClient, data: &Arc<RwLock<SharedTestData>>) {
        let value = client.read_x().read().await.unwrap();
        assert_eq!(value, 0);

        client.write_x(0xAA).write().await.unwrap();
        assert_eq!(data.read().unwrap().x, 0xAA);

        let value = client.read_x().read().await.unwrap();
        assert_eq!(value, 0xAA);

        let y = client.read_y().read().await.unwrap();
        assert_eq!(y, 0xBB);
        client.write_y(0xCC).write().await.unwrap();
        let y = client.read_y().read().await.unwrap();
        assert_eq!(y, 0xCC);
        assert_eq!(data.read().unwrap().y_changed, 1);

        client.write_y(0xCC).write().await.unwrap();
        assert_eq!(data.read().unwrap().y_changed, 1, "same value, no change");

        let mut expected_custom = CustomOwned {
            z: 123,
            inner: vec![
                InnerOwned {
                    u: 63,
                    v: "abc".into(),
                },
                InnerOwned {
                    u: 127,
                    v: "def".into(),
                },
            ],
        };
        let custom = client.read_custom().read().await.unwrap();
        assert_eq!(custom, expected_custom);

        expected_custom.z = 63;
        expected_custom.inner.push(InnerOwned {
            u: 255,
            v: "xy".to_string(),
        });
        client
            .write_custom(expected_custom.clone())
            .write()
            .await
            .unwrap();
        assert_eq!(data.read().unwrap().custom, expected_custom);
    }

    async fn observe(client: &StdClient) {
        let mut x = client.observe_x().await.unwrap();
        let mut y = client.observe_y().await.unwrap();
        client.write_x(5).write().await.unwrap();
        client.write_x(6).write().await.unwrap();
        // value_on_changed: notified only on change
        client.write_y(7).write().await.unwrap();
        client.write_y(7).write().await.unwrap();
        // updates are sent before the write is acknowledged
        assert_eq!(x.try_recv().unwrap(), Some(5));
        assert_eq!(x.try_recv().unwrap(), Some(6));
        assert_eq!(x.try_recv().unwrap(), None);
        assert_eq!(y.try_recv().unwrap(), Some(7));
        assert_eq!(y.try_recv().unwrap(), None);
    }

    async fn unimplemented(client: &StdClient) {
        let r = client.read_absent().read().await;
        assert!(matches!(
            remote_error_kind(r),
            ErrorKindOwned::Unimplemented
        ));
        let r = client.write_absent(0).write().await;
        assert!(matches!(
            remote_error_kind(r),
            ErrorKindOwned::Unimplemented
        ));
    }

    async fn user_errors(client: &StdClient, data: &Arc<RwLock<SharedTestData>>) {
        assert_eq!(
            user_error(client.read_mode().read().await),
            ModeError::NotSet
        );
        let r = client.write_mode(11).write().await;
        assert!(
            matches!(r, Err(CallError::Device(ModeError::TooBig))),
            "{r:?}"
        );
        assert_eq!(data.read().unwrap().mode, 0, "failed write is not applied");
        client.write_mode(5).write().await.unwrap();
        assert_eq!(client.read_mode().read().await.unwrap(), 5);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn multi_req_server() {
        let (device, data) = start_multi_req("properties/multi_req");
        let client = connect(&device).await;
        get_set_and_value_on_changed(&client, &data).await;
        unimplemented(&client).await;
        user_errors(&client, &data).await;

        let (x, absent) = (client.read_x(), client.read_absent())
            .multi_read()
            .await
            .unwrap();
        assert_eq!(x.unwrap(), 0xAA);
        assert!(matches!(
            absent.unwrap_err().kind,
            ErrorKindOwned::Unimplemented
        ));

        // N > 2 (CLT-15): any number of resources, not just a 2-tuple
        let (x, y, mode) = (client.read_x(), client.read_y(), client.read_mode())
            .multi_read()
            .await
            .unwrap();
        assert_eq!(x.unwrap(), 0xAA);
        assert_eq!(y.unwrap(), 0xCC);
        assert_eq!(mode.unwrap(), 5);

        // MultiWrite, also N > 2 (CLT-15); write_mode is skipped here, its CallError<ModeError> isn't a plain
        // PreparedWrite<Error> (out of scope for the tuple API, see docs/api/properties.md)
        let custom = CustomOwned {
            z: 7,
            inner: vec![],
        };
        let (wx, wy, wcustom) = (
            client.write_x(1),
            client.write_y(2),
            client.write_custom(custom.clone()),
        )
            .multi_write()
            .await
            .unwrap();
        wx.unwrap();
        wy.unwrap();
        wcustom.unwrap();
        assert_eq!(client.read_x().read().await.unwrap(), 1);
        assert_eq!(client.read_y().read().await.unwrap(), 2);
        // get_custom() is hardcoded in this test server (see handlers!), check the stored value instead
        assert_eq!(data.read().unwrap().custom, custom);

        observe(&client).await;
    }

    /// `read_array`/`write_array` over the `ch[]` array (CLT-15): list, range and mask indices, `Same`/
    /// `Different` values, and a per-item error (`BadIndex` past the array's length).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn array_multi_requests() {
        use wire_weaver_client::multi::{read_array, write_array};
        use ww_client_server::MultiIndexOwned;

        let (device, data) = start_multi_req("properties/array_multi_req");
        let client = connect(&device).await;

        // write different: one value per channel, in list order
        let r = write_array(
            MultiIndexOwned::List(vec![UNib32(0), UNib32(1), UNib32(2), UNib32(3)]),
            |i| client.ch(i).write_level((i as u8 + 1) * 10),
        )
        .await
        .unwrap();
        for item in r {
            item.unwrap();
        }
        assert_eq!(data.read().unwrap().ch_level, [10, 20, 30, 40]);

        // read range: same order back
        let r = read_array(MultiIndexOwned::Range(UNib32(0)..UNib32(4)), |i| {
            client.ch(i).read_level()
        })
        .await
        .unwrap();
        let r: Vec<u8> = r.into_iter().map(Result::unwrap).collect();
        assert_eq!(r, vec![10, 20, 30, 40]);

        // write same: one value to every selected channel (mask32: channels 0 and 2)
        let r = write_array(MultiIndexOwned::Mask32(0b101), |i| {
            client.ch(i).write_level(99)
        })
        .await
        .unwrap();
        for item in r {
            item.unwrap();
        }
        assert_eq!(data.read().unwrap().ch_level, [99, 20, 99, 40]);

        // per-item error: index 4 is past the array's length (0..4 valid)
        let r = read_array(MultiIndexOwned::Range(UNib32(2)..UNib32(5)), |i| {
            client.ch(i).read_level()
        })
        .await
        .unwrap();
        assert_eq!(r[0].as_ref().unwrap(), &99);
        assert_eq!(r[1].as_ref().unwrap(), &40);
        assert!(
            matches!(&r[2], Err(e) if matches!(e.kind, ErrorKindOwned::BadIndex)),
            "{r:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn single_req_server() {
        let (device, data) = start_single_req("properties/single_req");
        let client = connect(&device).await;
        get_set_and_value_on_changed(&client, &data).await;
        unimplemented(&client).await;
        user_errors(&client, &data).await;

        let r = (client.read_x(), client.read_absent()).multi_read().await;
        println!("multi_read on a server without multi_req: {r:?}");
        assert!(r.is_err(), "server without multi_req must refuse it");
        observe(&client).await;
    }

    /// tests_common makes the client strict: a device error outside the property's `#[errors(..)]` fails the test
    /// (API-32).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[should_panic(
        expected = "'Properties::mode' returned Device(Locked), which is not in its #[errors(TooBig, NotSet)]"
    )]
    async fn unlisted_write_error_fails_in_tests() {
        let (device, _data) = start_multi_req("properties/unlisted_write_error");
        let client = connect(&device).await;
        let _ = client.write_mode(99).write().await;
    }

    #[test]
    #[should_panic(expected = "'Properties::mode' returned Device(Locked)")]
    fn unlisted_write_error_fails_in_tests_promise() {
        let (device, _data) = start_multi_req("properties/unlisted_write_error_promise");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let client = connect_blocking(&device);
        let mut write = client.write_mode(99).write_promise("write_mode");
        tests_common::wait_promise(&mut write);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn timeout_async() {
        let (device, data) = start_multi_req("properties/timeout_async");
        let client = connect(&device).await;
        device.drop_requests(true);
        let r = client
            .read_x()
            .with_timeout(Duration::from_millis(50))
            .read()
            .await;
        assert!(matches!(r, Err(Error::Timeout)), "{r:?}");
        let r = client
            .write_x(1)
            .with_timeout(Duration::from_millis(50))
            .write()
            .await;
        assert!(matches!(r, Err(Error::Timeout)), "{r:?}");

        // device recovered: requests after a timeout are answered
        device.drop_requests(false);
        client.write_x(2).write().await.unwrap();
        assert_eq!(client.read_x().read().await.unwrap(), 2);
        assert_eq!(data.read().unwrap().x, 2);
    }

    #[test]
    fn timeout_blocking() {
        let (device, _data) = start_multi_req("properties/timeout_blocking");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let client = connect_blocking(&device);
        device.drop_requests(true);
        let r = client
            .read_x()
            .with_timeout(Duration::from_millis(50))
            .blocking_read();
        assert!(matches!(r, Err(Error::Timeout)), "{r:?}");
        let r = client
            .write_x(1)
            .with_timeout(Duration::from_millis(50))
            .blocking_write();
        assert!(matches!(r, Err(Error::Timeout)), "{r:?}");

        device.drop_requests(false);
        client.write_x(2).blocking_write().unwrap();
        assert_eq!(client.read_x().blocking_read().unwrap(), 2);
    }

    #[test]
    fn timeout_promise() {
        let (device, _data) = start_multi_req("properties/timeout_promise");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let client = connect_blocking(&device);
        device.drop_requests(true);
        let mut read = client
            .read_x()
            .with_timeout(Duration::from_millis(50))
            .read_promise("read_x");
        tests_common::wait_promise(&mut read);
        assert!(matches!(read.peek_error(), Some(Error::Timeout)), "{read}");
        let mut write = client
            .write_x(1)
            .with_timeout(Duration::from_millis(50))
            .write_promise("write_x");
        tests_common::wait_promise(&mut write);
        assert!(
            matches!(write.peek_error(), Some(Error::Timeout)),
            "{write}"
        );

        device.drop_requests(false);
        let mut read = client.read_x().read_promise("read_x");
        tests_common::wait_promise(&mut read);
        assert_eq!(read.take_ready(), Some(0));
    }

    #[test]
    fn user_error_promise() {
        let (device, _data) = start_multi_req("properties/user_error_promise");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let client = connect_blocking(&device);
        let mut write = client.write_mode(5).write_promise("write_mode");
        tests_common::wait_promise(&mut write);
        assert_eq!(write.take_ready(), Some(()), "{write}");
        let mut write = client.write_mode(11).write_promise("write_mode");
        tests_common::wait_promise(&mut write);
        let Some(CallError::Device(ModeError::TooBig)) = write.peek_error() else {
            panic!("expected deserialized user error, got {write}");
        };

        let mut read = client.read_mode().read_promise("read_mode");
        tests_common::wait_promise(&mut read);
        assert_eq!(read.take_ready(), Some(5), "{read}");
    }
}

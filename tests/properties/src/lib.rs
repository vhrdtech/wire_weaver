#[cfg(test)]
mod tests {
    use properties_api::{CustomOwned, InnerOwned};
    use std::sync::{Arc, RwLock};
    use std::time::Duration;
    use tests_common::DummyTx;
    use tokio::sync::mpsc;
    use wire_weaver::shrink_wrap::RefVec;
    use wire_weaver_client::{Commander, Error, MultiRead};
    use ww_client_server::ErrorKindOwned;

    #[derive(Default)]
    struct SharedTestData {
        x: u8,
        y_changed: u8,
        custom: CustomOwned,
    }

    mod no_std_sync_server {
        use super::*;
        use properties_api::{Custom, Inner};
        use std::sync::{Arc, RwLock};
        use tests_common::TestProcessEvents;
        use wire_weaver::prelude::*;
        use wire_weaver::{MessageSink, SetResult, Unimplemented};

        pub struct NoStdSyncServer {
            pub data: Arc<RwLock<SharedTestData>>,
            pub y: u8,
        }

        impl NoStdSyncServer {
            fn set_x(&mut self, value: u8) -> SetResult<()> {
                self.data.write().unwrap().x = value;
                Set
            }

            fn get_x(&mut self) -> GetResult<u8, ()> {
                Value(self.data.read().unwrap().x)
            }

            fn changed_y(&mut self) {
                self.data.write().unwrap().y_changed += 1;
            }

            fn get_custom(&mut self) -> GetResult<Custom<'_>, ()> {
                Value(Custom {
                    z: 123,
                    inner: RefVec::Slice {
                        slice: &[Inner { u: 63, v: "abc" }, Inner { u: 127, v: "def" }],
                    },
                })
            }

            fn set_custom(&mut self, custom: Custom<'_>) -> SetResult<()> {
                let custom = custom.make_owned();
                self.data.write().unwrap().custom = custom;
                Set
            }

            fn set_absent(&mut self, _value: u8) -> SetResult<()> {
                Unimplemented.into()
            }

            fn get_absent(&mut self) -> GetResult<u8, ()> {
                Unimplemented.into()
            }
        }

        mod api_impl {
            wire_weaver::ww_codegen!(
                properties_api :: Properties for super::NoStdSyncServer,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "x=get_set, y=value_on_changed",
                multi_req = true
                // debug_to_file = "../target/tests_properties_server.rs" // uncomment if you want to see the resulting AST and generated code
            );
        }

        impl TestProcessEvents for NoStdSyncServer {
            fn process_request_bytes<'a>(
                &mut self,
                bytes: &[u8],
                scratch_args: &'a mut [u8],
                scratch_event: &'a mut [u8],
                scratch_err: &'a mut [u8],
                msg_tx: &mut impl MessageSink,
            ) -> Result<&'a [u8], ShrinkWrapError> {
                self.process_request_bytes(bytes, scratch_args, scratch_event, scratch_err, msg_tx)
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
        tracing_subscriber::fmt::init();
        let (cmd_tx, cmd_rx) = mpsc::channel(128);
        let data = Arc::new(RwLock::new(SharedTestData::default()));

        let data_clone = data.clone();
        let server = no_std_sync_server::NoStdSyncServer {
            data: data_clone,
            y: 0xBB,
        };
        tokio::spawn(async move {
            tests_common::test_event_loop(cmd_rx, server, DummyTx {}).await;
        });

        let cmd = Commander::new(cmd_tx);
        let client = std_client::StdClient { cmd };
        tokio::time::sleep(Duration::from_millis(10)).await;

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
        assert_eq!(data.read().unwrap().y_changed, 1);

        let absent = client.read_absent().read().await;
        let Error::RemoteError(e) = absent.unwrap_err() else {
            panic!("Expected RemoteError");
        };
        assert!(matches!(e.kind, ErrorKindOwned::Unimplemented));

        let set_absent = client.write_absent(0).write().await;
        let Error::RemoteError(e) = set_absent.unwrap_err() else {
            panic!("Expected RemoteError");
        };
        assert!(matches!(e.kind, ErrorKindOwned::Unimplemented));

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

        let multi_read = (client.read_x(), client.read_absent())
            .multi_read()
            .await
            .unwrap();
        let x = multi_read.0.unwrap();
        // let y = multi_read.1.unwrap();
        let e = multi_read.1.unwrap_err();
        assert_eq!(x, 0xAA);
        // assert_eq!(y, 0xCC);
        assert!(matches!(e.kind, ErrorKindOwned::Unimplemented));
    }
}

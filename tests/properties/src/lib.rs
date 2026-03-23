#[cfg(test)]
mod tests {
    use std::sync::{Arc, RwLock};
    use std::time::Duration;
    use tests_common::DummyTx;
    use tokio::sync::mpsc;
    use wire_weaver::ww_version::{FullVersionOwned, VersionOwned};
    use wire_weaver_client_common::{CommandSender, DeviceFilter, OnError};

    #[derive(Default)]
    struct SharedTestData {
        plain: u8,
    }

    mod no_std_sync_server {
        use super::*;
        use std::sync::{Arc, RwLock};
        use tests_common::TestProcessEvents;
        use wire_weaver::prelude::ShrinkWrapError;
        use wire_weaver::MessageSink;

        pub struct NoStdSyncServer {
            pub data: Arc<RwLock<SharedTestData>>,
        }

        impl NoStdSyncServer {
            fn set_plain(&mut self, value: u8) {
                self.data.write().unwrap().plain = value;
            }

            fn get_plain(&mut self) -> u8 {
                self.data.read().unwrap().plain
            }
        }

        mod api_impl {
            wire_weaver::ww_codegen!(
                "../properties_api" :: Properties for NoStdSyncServer,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                introspect = false,
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

    mod std_async_client {
        use wire_weaver_client_common::CommandSender;

        pub struct StdAsyncClient {
            pub cmd_tx: CommandSender,
        }

        mod api_client {
            wire_weaver::ww_codegen!(
                "../properties_api" :: Properties for StdAsyncClient,
                client = "full_client",
                no_alloc = false,
                use_async = true,
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
        let (transport_cmd_tx, transport_cmd_rx) = mpsc::unbounded_channel();
        let data = Arc::new(RwLock::new(SharedTestData::default()));

        let data_clone = data.clone();
        let server = no_std_sync_server::NoStdSyncServer { data: data_clone };
        tokio::spawn(async move {
            tests_common::test_event_loop(transport_cmd_rx, server, DummyTx {}).await;
        });

        let mut cmd_tx = CommandSender::new(transport_cmd_tx);
        cmd_tx
            .connect(
                DeviceFilter::vhrd_usb_can(),
                FullVersionOwned::new("test".into(), VersionOwned::new(0, 1, 0)),
                OnError::ExitImmediately,
            )
            .await
            .expect("connect");
        let client = std_async_client::StdAsyncClient { cmd_tx };
        tokio::time::sleep(Duration::from_millis(10)).await;

        let value = client.read_plain().read().await.unwrap();
        assert_eq!(value, 0);

        client.write_plain(0xAA).write().await.unwrap();
        assert_eq!(data.read().unwrap().plain, 0xAA);

        let value = client.read_plain().read().await.unwrap();
        assert_eq!(value, 0xAA);
    }
}

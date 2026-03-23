#[cfg(test)]
mod tests {
    use methods_api::UserDefinedOwned;
    use std::sync::{Arc, RwLock};
    use std::time::Duration;
    use tests_common::DummyTx;
    use tokio::sync::mpsc;
    use wire_weaver::prelude::*;
    use wire_weaver_client_common::ww_version::{FullVersionOwned, VersionOwned};
    use wire_weaver_client_common::{CommandSender, DeviceFilter, OnError};

    #[derive(Default)]
    struct SharedTestData {
        no_args_called: bool,
        one_plain_arg: u8,
    }

    mod no_std_sync_server {
        use super::*;
        use methods_api::UserDefined;
        use tests_common::TestProcessEvents;
        use wire_weaver::MessageSink;

        pub struct NoStdSyncServer {
            pub data: Arc<RwLock<SharedTestData>>,
        }

        impl NoStdSyncServer {
            fn no_args(&mut self, _msg_tx: &mut impl MessageSink) {
                self.data.write().unwrap().no_args_called = true;
            }

            fn one_plain_arg(&mut self, _msg_tx: &mut impl MessageSink, value: u8) {
                self.data.write().unwrap().one_plain_arg = value;
            }

            fn plain_return(&mut self, _msg_tx: &mut impl MessageSink) -> u8 {
                0xAA
            }

            fn user_arg(&mut self, _msg_tx: &mut impl MessageSink, u: UserDefined<'_>) {
                assert_eq!(u.a, 123);
                let mut iter = u.b.into_iter();
                assert_eq!(iter.next(), Some(&1));
                assert_eq!(iter.next(), Some(&2));
                assert_eq!(iter.next(), Some(&3));
                assert_eq!(iter.next(), None);
            }

            fn user_defined_return(&mut self, _msg_tx: &mut impl MessageSink) -> UserDefined<'_> {
                UserDefined {
                    a: 37,
                    b: RefVec::new_bytes(&[1, 2, 3]),
                }
            }
        }

        mod api_impl {
            wire_weaver::ww_codegen!(
                "../methods_api" :: Methods for NoStdSyncServer,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                introspect = false,
                debug_to_file = "../../target/tests_methods_server.rs" // uncomment if you want to see the resulting AST and generated code
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
                "../methods_api" :: Methods for StdAsyncClient,
                client = "full_client",
                no_alloc = false,
                use_async = true,
                debug_to_file = "../../target/tests_methods_client.rs"
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
        let mut client = std_async_client::StdAsyncClient { cmd_tx };
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Call as async
        client.no_args().call().await.unwrap();
        assert!(data.read().unwrap().no_args_called);

        // Call forget
        data.write().unwrap().no_args_called = false;
        client.no_args().call_forget().unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(data.read().unwrap().no_args_called);

        // Call via Promise
        data.write().unwrap().no_args_called = false;
        let mut promise = client.no_args().call_promise("marker");
        tokio::task::spawn_blocking(move || {
            promise.sync_poll();
            std::thread::sleep(Duration::from_millis(10));
            promise.sync_poll();
            std::thread::sleep(Duration::from_millis(10));
            assert_eq!(promise.take_ready(), Some(()));
        })
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(data.read().unwrap().no_args_called);

        client.one_plain_arg(0xCC).call().await.unwrap();
        assert_eq!(data.read().unwrap().one_plain_arg, 0xCC);

        let value = client.plain_return().call().await.unwrap();
        assert_eq!(value, 0xAA);

        client
            .user_arg(UserDefinedOwned {
                a: 123,
                b: vec![1, 2, 3],
            })
            .call()
            .await
            .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use methods_api::UserDefinedOwned;
    use std::sync::{Arc, RwLock};
    use std::time::Duration;
    use tests_common::TestDevice;
    use wire_weaver_client::Error;
    use ww_client_server::ErrorKindOwned;

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
        use wire_weaver::prelude::*;

        pub struct NoStdSyncServer {
            pub data: Arc<RwLock<SharedTestData>>,
        }

        impl NoStdSyncServer {
            fn no_args(&mut self, _msg_tx: &mut impl MessageSink) -> RpcResult<()> {
                self.data.write().unwrap().no_args_called = true;
                Ready(())
            }

            fn one_plain_arg(
                &mut self,
                _msg_tx: &mut impl MessageSink,
                value: u8,
            ) -> RpcResult<()> {
                self.data.write().unwrap().one_plain_arg = value;
                Ready(())
            }

            fn plain_return(&mut self, _msg_tx: &mut impl MessageSink) -> RpcResult<u8> {
                Ready(0xAA)
            }

            fn user_arg(
                &mut self,
                _msg_tx: &mut impl MessageSink,
                u: UserDefined<'_>,
            ) -> RpcResult<()> {
                assert_eq!(u.a, 123);
                let mut iter = u.b[..].iter();
                assert_eq!(iter.next(), Some(&1));
                assert_eq!(iter.next(), Some(&2));
                assert_eq!(iter.next(), Some(&3));
                assert_eq!(iter.next(), None);
                Ready(())
            }

            fn user_defined_return(
                &mut self,
                _msg_tx: &mut impl MessageSink,
            ) -> RpcResult<UserDefined<'_>> {
                Ready(UserDefined {
                    a: 37,
                    b: RefVec::new_bytes(&[1, 2, 3]),
                })
            }

            fn deferred(&mut self, _msg_tx: &mut impl MessageSink) -> RpcResult<u8> {
                Deferred
            }

            fn deferred_unit(&mut self, _msg_tx: &mut impl MessageSink) -> RpcResult<()> {
                Deferred
            }

            fn absent(&mut self, _msg_tx: &mut impl MessageSink) -> RpcResult<()> {
                Unimplemented.into()
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                methods_api :: Methods for super::NoStdSyncServer,
                server = true, no_alloc = true, use_async = false,
                method_model = "deferred=deferred, deferred_unit=deferred, _=immediate",
                property_model = "_=get_set",
                // debug_to_file = "../../target/tests_methods_server.rs" // uncomment if you want to see the resulting AST and generated code
            );
        }

        impl TestProcessEvents for NoStdSyncServer {
            fn process_request_bytes<'a>(
                &mut self,
                bytes: &[u8],
                scratch: &'a mut [u8],
                msg_tx: &mut impl MessageSink,
            ) -> Result<&'a [u8], ShrinkWrapError> {
                self.process_request_bytes(bytes, scratch, msg_tx)
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
                methods_api :: Methods for super::StdClient,
                client = "std_client",
                // debug_to_file = "../../target/tests_methods_client.rs"
            );
        }
    }

    use std_client::StdClient;

    fn start(path: &str) -> (TestDevice, Arc<RwLock<SharedTestData>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let data = Arc::new(RwLock::new(SharedTestData::default()));
        let server = no_std_sync_server::NoStdSyncServer { data: data.clone() };
        let device = tests_common::start_device(
            path,
            server,
            methods_api::METHODS_FULL_GID,
            no_std_sync_server::api_impl::api_hash(),
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn calls() {
        let (device, data) = start("methods/calls");
        // methods with arguments take &mut self
        let mut client = connect(&device).await;

        // Call as async
        client.no_args().call().await.unwrap();
        assert!(data.read().unwrap().no_args_called);

        // Call forget
        data.write().unwrap().no_args_called = false;
        client.no_args().call_forget().await.unwrap();
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

        let value = client.user_defined_return().call().await.unwrap();
        assert_eq!(
            value,
            UserDefinedOwned {
                a: 37,
                b: vec![1, 2, 3]
            }
        );
    }

    #[test]
    fn calls_blocking_and_promise() {
        let (device, data) = start("methods/calls_blocking_and_promise");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut client = connect_blocking(&device);

        client.no_args().blocking_call().unwrap();
        assert!(data.read().unwrap().no_args_called);
        assert_eq!(client.plain_return().blocking_call().unwrap(), 0xAA);

        data.write().unwrap().one_plain_arg = 0;
        client.one_plain_arg(7).blocking_call_forget().unwrap();
        std::thread::sleep(Duration::from_millis(10));
        assert_eq!(data.read().unwrap().one_plain_arg, 7);

        let mut promise = client
            .user_defined_return()
            .call_promise("user_defined_return");
        tests_common::wait_promise(&mut promise);
        assert_eq!(
            promise.take_ready(),
            Some(UserDefinedOwned {
                a: 37,
                b: vec![1, 2, 3]
            })
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unimplemented() {
        let (device, _data) = start("methods/unimplemented");
        let client = connect(&device).await;
        let r = client.absent().call().await;
        let Err(Error::RemoteError(e)) = r else {
            panic!("expected RemoteError, got {r:?}");
        };
        assert!(matches!(e.kind, ErrorKindOwned::Unimplemented));
        // and the connection is still usable
        client.no_args().call().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn timeout_async() {
        let (device, _data) = start("methods/timeout_async");
        let client = connect(&device).await;
        device.drop_requests(true);
        let r = client
            .plain_return()
            .with_timeout(Duration::from_millis(50))
            .call()
            .await;
        assert!(matches!(r, Err(Error::Timeout)), "{r:?}");

        device.drop_requests(false);
        assert_eq!(client.plain_return().call().await.unwrap(), 0xAA);
    }

    #[test]
    fn timeout_blocking() {
        let (device, _data) = start("methods/timeout_blocking");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let client = connect_blocking(&device);
        device.drop_requests(true);
        let r = client
            .plain_return()
            .with_timeout(Duration::from_millis(50))
            .blocking_call();
        assert!(matches!(r, Err(Error::Timeout)), "{r:?}");

        device.drop_requests(false);
        assert_eq!(client.plain_return().blocking_call().unwrap(), 0xAA);
    }

    #[test]
    fn timeout_promise() {
        let (device, _data) = start("methods/timeout_promise");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let client = connect_blocking(&device);
        device.drop_requests(true);
        let mut promise = client
            .plain_return()
            .with_timeout(Duration::from_millis(50))
            .call_promise("plain_return");
        tests_common::wait_promise(&mut promise);
        assert!(
            matches!(promise.peek_error(), Some(Error::Timeout)),
            "{promise}"
        );

        device.drop_requests(false);
        let mut promise = client.plain_return().call_promise("plain_return");
        tests_common::wait_promise(&mut promise);
        assert_eq!(promise.take_ready(), Some(0xAA));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn deferred_not_answered_times_out() {
        let (device, _data) = start("methods/deferred");
        let client = connect(&device).await;
        let r = client
            .deferred()
            .with_timeout(Duration::from_millis(50))
            .call()
            .await;
        assert!(matches!(r, Err(Error::Timeout)), "{r:?}");
        let r = client
            .deferred_unit()
            .with_timeout(Duration::from_millis(50))
            .call()
            .await;
        assert!(matches!(r, Err(Error::Timeout)), "{r:?}");
        // other requests are still answered
        assert_eq!(client.plain_return().call().await.unwrap(), 0xAA);
    }

    #[test]
    fn deferred_reply_event() {
        use wire_weaver::prelude::DeserializeShrinkWrap;
        let mut args = [0u8; 16];
        let mut scratch_event = [0u8; 16];
        let bytes = no_std_sync_server::NoStdSyncServer::deferred_ser_return_event(
            &mut args,
            &mut scratch_event,
            5,
            0x42,
        )
        .unwrap();
        let event = ww_client_server::Event::from_ww_bytes(bytes).unwrap();
        assert_eq!(event.seq.0, 5);
        let Ok(ww_client_server::EventKind::Value { data }) = event.result else {
            panic!("expected Value");
        };
        assert_eq!(data.0, &[0x42]);

        let bytes = no_std_sync_server::NoStdSyncServer::deferred_unit_ser_return_event(
            &mut args,
            &mut scratch_event,
            6,
        )
        .unwrap();
        let event = ww_client_server::Event::from_ww_bytes(bytes).unwrap();
        assert_eq!(event.seq.0, 6);
    }
}

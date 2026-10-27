//! Stream functions whose error type borrows (API-35): the handlers have the signature `ww api scaffold` prints,
//! `RpcResult<Result<(), OpenError<'_>>>`, and return an error borrowed from the server. The sync server has plain and
//! counted streams, the async one adds a reliable stream; both open a stream and refuse one with that error.

#[cfg(test)]
mod tests {
    use streams_borrowed_error_api::{InnerError, OpenError, OpenErrorOwned};
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use ww_stream::store::RamRing;
    use ww_stream::tx::{Counted, Reliable};

    const OWNER: &str = "other host";

    fn busy(owner: &str) -> OpenError<'_> {
        OpenError::Busy { by: owner }
    }

    fn inner(message: &str) -> OpenError<'_> {
        OpenError::Inner(InnerError::Message(message))
    }

    mod sync_server {
        use super::*;
        use tests_common::TestProcessEvents;

        pub struct Server {
            /// Errors borrow from here
            pub owner: String,
            pub counted_stream: Counted,
            pub stream_sessions: u32,
        }

        impl Server {
            fn plain(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                fail: bool,
            ) -> RpcResult<Result<(), OpenError<'_>>> {
                Ready(if fail { Err(busy(&self.owner)) } else { Ok(()) })
            }

            fn close_plain(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn sideband_plain(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }

            fn counted(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                fail: bool,
            ) -> RpcResult<Result<(), OpenError<'_>>> {
                Ready(if fail {
                    Err(inner(&self.owner))
                } else {
                    Ok(())
                })
            }

            fn close_counted(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn stream_counted(&mut self) -> &mut Counted {
                &mut self.counted_stream
            }

            fn new_stream_session(&mut self) -> u32 {
                self.stream_sessions = self.stream_sessions.wrapping_add(1);
                self.stream_sessions
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                streams_borrowed_error_api :: BorrowedError for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                // debug_to_file = "../../target/tests_streams_borrowed_error_sync_server.rs"
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

    mod async_server {
        use super::*;
        use tests_common::TestProcessEventsAsync;

        pub struct Server {
            /// Errors borrow from here
            pub owner: String,
            pub counted_stream: Counted,
            pub reliable_stream: Reliable<RamRing<4096>>,
            pub stream_sessions: u32,
        }

        impl Server {
            async fn plain(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                fail: bool,
            ) -> RpcResult<Result<(), OpenError<'_>>> {
                Ready(if fail { Err(busy(&self.owner)) } else { Ok(()) })
            }

            async fn close_plain(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

            async fn sideband_plain(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }

            async fn counted(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                fail: bool,
            ) -> RpcResult<Result<(), OpenError<'_>>> {
                Ready(if fail {
                    Err(inner(&self.owner))
                } else {
                    Ok(())
                })
            }

            async fn close_counted(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

            async fn stream_counted(&mut self) -> &mut Counted {
                &mut self.counted_stream
            }

            async fn reliable(
                &mut self,
                _cx: &mut Context<'_, impl EventOut>,
                fail: bool,
            ) -> RpcResult<Result<(), OpenError<'_>>> {
                Ready(if fail { Err(busy(&self.owner)) } else { Ok(()) })
            }

            async fn close_reliable(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

            async fn stream_reliable(&mut self) -> &mut Reliable<RamRing<4096>> {
                &mut self.reliable_stream
            }

            fn new_stream_session(&mut self) -> u32 {
                self.stream_sessions = self.stream_sessions.wrapping_add(1);
                self.stream_sessions
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                streams_borrowed_error_api :: BorrowedErrorReliable for super::Server,
                server = true, no_alloc = true, use_async = true,
                method_model = "_=immediate",
                property_model = "_=get_set",
                // debug_to_file = "../../target/tests_streams_borrowed_error_async_server.rs"
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

            async fn poll(&mut self, out: &mut impl EventOut, now_ms: u64) {
                let mut buf = [0u8; 256];
                _ = self.poll_streams(out, now_ms, &mut buf, 4096).await;
            }
        }
    }

    mod client {
        use wire_weaver_client::{ClientConfig, Commander, WwClient};

        pub struct SyncClient {
            pub cmd: Commander,
        }

        impl WwClient for SyncClient {
            fn default_config() -> ClientConfig {
                ClientConfig::default()
            }

            fn from_cmd(cmd: Commander) -> Self {
                Self { cmd }
            }
        }

        pub struct AsyncClient {
            pub cmd: Commander,
        }

        impl WwClient for AsyncClient {
            fn default_config() -> ClientConfig {
                ClientConfig::default()
            }

            fn from_cmd(cmd: Commander) -> Self {
                Self { cmd }
            }
        }

        mod sync_api_client {
            wire_weaver::ww_codegen!(
                streams_borrowed_error_api :: BorrowedError for super::SyncClient,
                client = "std_client",
            );
        }

        mod async_api_client {
            wire_weaver::ww_codegen!(
                streams_borrowed_error_api :: BorrowedErrorReliable for super::AsyncClient,
                client = "std_client",
            );
        }
    }

    use client::{AsyncClient, SyncClient};

    fn start_sync(path: &str) -> TestDevice {
        let _ = tracing_subscriber::fmt::try_init();
        let server = sync_server::Server {
            owner: OWNER.into(),
            counted_stream: Counted::new(),
            stream_sessions: 0,
        };
        tests_common::start_device(
            path,
            server,
            streams_borrowed_error_api::BORROWED_ERROR_FULL_GID,
            sync_server::api_impl::api_hash(),
        )
    }

    fn start_async(path: &str) -> TestDevice {
        let _ = tracing_subscriber::fmt::try_init();
        let server = async_server::Server {
            owner: OWNER.into(),
            counted_stream: Counted::new(),
            reliable_stream: Reliable::new(RamRing::new()),
            stream_sessions: 0,
        };
        tests_common::start_device_async(
            path,
            server,
            streams_borrowed_error_api::BORROWED_ERROR_RELIABLE_FULL_GID,
            async_server::api_impl::api_hash(),
        )
    }

    fn busy_owned() -> OpenErrorOwned {
        OpenErrorOwned::Busy { by: OWNER.into() }
    }

    fn inner_owned() -> OpenErrorOwned {
        OpenErrorOwned::Inner(streams_borrowed_error_api::InnerErrorOwned::Message(
            OWNER.into(),
        ))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sync_server_opens_and_refuses() {
        let device = start_sync("streams_borrowed_error/sync");
        let client = SyncClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");

        let r = client.plain(true).call().await.unwrap();
        assert_eq!(r.err(), Some(busy_owned()));
        assert!(client.plain(false).call().await.unwrap().is_ok());

        let r = client.counted(true).call().await.unwrap();
        assert_eq!(r.err(), Some(inner_owned()));
        assert!(client.counted(false).call().await.unwrap().is_ok());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn async_server_opens_and_refuses() {
        let device = start_async("streams_borrowed_error/async");
        let client = AsyncClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");

        let r = client.plain(true).call().await.unwrap();
        assert_eq!(r.err(), Some(busy_owned()));
        assert!(client.plain(false).call().await.unwrap().is_ok());

        let r = client.counted(true).call().await.unwrap();
        assert_eq!(r.err(), Some(inner_owned()));
        assert!(client.counted(false).call().await.unwrap().is_ok());

        let r = client.reliable(true).call().await.unwrap();
        assert_eq!(r.err(), Some(busy_owned()));
        assert!(client.reliable(false).call().await.unwrap().is_ok());
    }
}

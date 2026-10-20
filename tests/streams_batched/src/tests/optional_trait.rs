//! Batched streams inside a left-out `#[optional]` `ww_impl!` (API-54): with `omit_if`, `poll_streams` keeps the
//! signature of the variant that has them and sends nothing, without their handlers; with `omit` it isn't generated.

use super::*;
use tests_common::TestProcessEventsAsync;

/// `omit_if(all())`: the `streams` trait is left out, its handlers are cfg'd out with the same condition.
mod cfg_server {
    use super::*;

    pub struct CfgServer;

    impl CfgServer {
        async fn ping(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<u8> {
            RpcResult::Ready(1)
        }
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            streams_batched_api :: BatchedBoard for super::CfgServer,
            server = true, no_alloc = true, use_async = true,
            omit_if(all()) = ["streams"],
        );
    }

    impl TestProcessEventsAsync for CfgServer {
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
            let mut scratch = Scratch::<{ scratch_bytes::<Timed<Sample>>(N) }>::new();
            self.poll_streams(out, now_ms, &mut buf, 4096, &mut scratch)
                .await
                .unwrap();
        }
    }
}

/// `omit`: no polled streams at all, so no `poll_streams`.
mod lean_server {
    use super::*;

    pub struct LeanServer;

    impl LeanServer {
        async fn ping(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<u8> {
            RpcResult::Ready(1)
        }
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            streams_batched_api :: BatchedBoard for super::LeanServer,
            server = true, no_alloc = true, use_async = true,
            omit = ["streams"],
        );
    }

    impl TestProcessEventsAsync for LeanServer {
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

mod board_client {
    use wire_weaver_client::{ClientConfig, Commander, WwClient};

    pub struct BoardClient {
        pub cmd: Commander,
    }

    impl WwClient for BoardClient {
        fn default_config() -> ClientConfig {
            ClientConfig::default()
        }

        fn from_cmd(cmd: Commander) -> Self {
            Self { cmd }
        }
    }

    mod api_client {
        wire_weaver::ww_codegen!(
            streams_batched_api :: BatchedBoard for super::BoardClient,
            client = "std_client",
        );
    }
}

use board_client::BoardClient;

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn streams_of_an_omitted_trait_are_not_polled() {
    let _ = tracing_subscriber::fmt::try_init();
    for (path, device) in [
        (
            "streams_batched/optional_trait_cfg",
            tests_common::start_device_async(
                "streams_batched/optional_trait_cfg",
                cfg_server::CfgServer,
                streams_batched_api::STREAMS_BATCHED_FULL_GID,
                cfg_server::api_impl::api_hash(),
            ),
        ),
        (
            "streams_batched/optional_trait_lean",
            tests_common::start_device_async(
                "streams_batched/optional_trait_lean",
                lean_server::LeanServer,
                streams_batched_api::STREAMS_BATCHED_FULL_GID,
                lean_server::api_impl::api_hash(),
            ),
        ),
    ] {
        let client = BoardClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect(path);
        assert_eq!(client.ping().call().await.unwrap(), 1, "{path}");
        assert_not_implemented(
            client.streams().produce(1, 0).call().await,
            "streams.produce",
        );
    }
}

//! Clients of a device behind the ww hub when the hub goes (CLT-40) or hands over to a newer one (SRV-30). The device
//! is served in the hub's process (`inproc:` key): in the tests' own process for a hub that is shut down and bound
//! again, in `hub_dev` for a hub that is killed and started again by the client.

use std::sync::{Arc, Mutex};

use tests_common::TestDevice;
use wire_weaver::prelude::*;
use ww_stream::tx::Counted;

/// What the device saw.
#[derive(Default, Debug)]
pub struct Seen {
    /// Every `add` call's arguments, in order: a call sent again would show up twice
    pub adds: Vec<(u32, u32)>,
}

pub mod server {
    use super::*;
    use tests_common::TestProcessEventsAsync;

    pub struct Server {
        pub seen: Arc<Mutex<Seen>>,
        pub ticks_stream: Counted,
        pub next_tick: u32,
        pub stream_sessions: u32,
    }

    impl Server {
        async fn add(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            a: u32,
            b: u32,
        ) -> RpcResult<u32> {
            self.seen.lock().unwrap().adds.push((a, b));
            RpcResult::Ready(a.wrapping_add(b))
        }

        async fn ticks(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
            RpcResult::Ready(())
        }

        async fn close_ticks(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

        async fn stream_ticks(&mut self) -> &mut Counted {
            &mut self.ticks_stream
        }

        async fn emit(&mut self, cx: &mut Context<'_, impl EventOut>, count: u32) -> RpcResult<()> {
            let ser = api_impl::stream_data_ser();
            for _ in 0..count {
                let tick = self.next_tick;
                self.next_tick += 1;
                ser.ticks_send(&mut self.ticks_stream, &tick, 1, None, cx)
                    .await
                    .unwrap();
            }
            RpcResult::Ready(())
        }

        fn new_stream_session(&mut self) -> u32 {
            self.stream_sessions = self.stream_sessions.wrapping_add(1);
            self.stream_sessions
        }
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            hub_reconnect_api :: HubReconnect for super::Server,
            server = true, no_alloc = true, use_async = true,
            method_model = "_=immediate",
            property_model = "_=get_set",
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

pub mod client {
    use wire_weaver_client::{ClientConfig, Commander, WwClient};

    pub struct Client {
        pub cmd: Commander,
    }

    impl WwClient for Client {
        fn default_config() -> ClientConfig {
            ClientConfig::default()
        }

        fn from_cmd(cmd: Commander) -> Self {
            Self { cmd }
        }
    }

    mod api_client {
        wire_weaver::ww_codegen!(
            hub_reconnect_api :: HubReconnect for super::Client,
            client = "std_client",
        );
    }
}

/// The device, served in this process at `inproc:<path>`.
pub fn start_device(path: &str) -> (TestDevice, Arc<Mutex<Seen>>) {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let device = tests_common::start_device_async(
        path,
        server::Server {
            seen: seen.clone(),
            ticks_stream: Counted::new(),
            next_tick: 0,
            stream_sessions: 0,
        },
        hub_reconnect_api::HUB_RECONNECT_FULL_GID,
        server::api_impl::api_hash(),
    );
    (device, seen)
}

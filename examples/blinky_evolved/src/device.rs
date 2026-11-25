//! The blinky firmware of both versions, simulated on a PC and served in process: [serve_w1_0] runs the firmware
//! already in the field (`blinky_api` 0.1.0 at w1.0), [serve_w1_1] the new one (0.1.1 at w1.1, with `led_toggle`).
//! The LED is a `bool`. Each serves the WebSocket clients of a [WsPort], made with `in_process::ws_device(path)`
//! before a host connects to `path`.

use std::future::Future;
use std::pin::pin;

use wire_weaver::prelude::*;
use wire_weaver::{MessageSink, WireWeaverAsyncApiBackend};
use wire_weaver_client::in_process::{TokioClock, WsPort};
use ww_device::LinkConfig;
use ww_device::ws::{IoSocket, WsBuffers, WsConnection, ws_server};

/// Longest message either side sends.
const MAX_MESSAGE_LEN: usize = 1024;

/// Serves firmware 0.1.0 to the clients of `port` until `stop` resolves; returns whether the LED is on.
pub async fn serve_w1_0(port: WsPort, stop: impl Future<Output = ()>) -> bool {
    let mut firmware = w1_0::Firmware { led: false };
    serve(port, w1_0::link_config(), &mut firmware, stop).await;
    firmware.led
}

/// Serves firmware 0.1.1 to the clients of `port` until `stop` resolves; returns whether the LED is on.
pub async fn serve_w1_1(port: WsPort, stop: impl Future<Output = ()>) -> bool {
    let mut firmware = w1_1::Firmware { led: false };
    serve(port, w1_1::link_config(), &mut firmware, stop).await;
    firmware.led
}

/// Serves `firmware` to the clients of `port`, one at a time, until `stop` resolves. The futures are not `Send`:
/// run it on a current-thread runtime or in a `LocalSet`.
async fn serve<B: WireWeaverAsyncApiBackend<Medium = ()>>(
    port: WsPort,
    link_config: LinkConfig<'static>,
    firmware: &mut B,
    stop: impl Future<Output = ()>,
) {
    let conn = WsConnection::new(IoSocket::new(port));
    let mut buffers = Box::new(WsBuffers::<MAX_MESSAGE_LEN>::new());
    let mut server = ws_server(link_config, &conn, TokioClock::new(), &mut buffers);
    let mut stop = pin!(stop);
    loop {
        tokio::select! {
            ready = server.wait() => {
                _ = server.handle(ready, firmware).await;
            }
            () = &mut stop => return,
        }
    }
}

/// The firmware already in the field: `blinky_api` 0.1.0, wire w1.0.
mod w1_0 {
    use super::*;

    pub(super) struct Firmware {
        pub(super) led: bool,
    }

    pub(super) fn link_config() -> LinkConfig<'static> {
        LinkConfig::new(
            blinky_api::BLINKY_API_FULL_GID,
            blinky_api::BLINKY_API_WIRE_VERSION,
            server_impl::api_hash(),
            server_impl::image_facts(),
            ww_client_server::COMPACT_VERSION,
        )
    }

    impl WireWeaverAsyncApiBackend for Firmware {
        type Medium = ();

        async fn process_bytes<'a>(
            &mut self,
            out: &mut EventWriter<'_, impl MessageSink>,
            medium: (),
            data: &[u8],
            scratch: &'a mut [u8],
        ) -> Result<&'a [u8], shrink_wrap::Error> {
            self.process_request_bytes(data, scratch, out, medium).await
        }

        fn version(&self) -> FullVersion<'_> {
            blinky_api::BLINKY_API_FULL_GID
        }
    }

    mod server_impl {
        wire_weaver::ww_codegen!(
            blinky_api :: BlinkyApi for super::Firmware,
            server = true, no_alloc = true, use_async = true,
            method_model = "_=immediate",
            property_model = "_=get_set",
            introspect = "with_docs",
        );
    }

    impl Firmware {
        async fn led_on(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
            self.led = true;
            Ready(())
        }

        async fn led_off(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
            self.led = false;
            Ready(())
        }
    }
}

/// The new firmware: `blinky_api` 0.1.1, wire w1.1.
mod w1_1 {
    use super::*;

    pub(super) struct Firmware {
        pub(super) led: bool,
    }

    pub(super) fn link_config() -> LinkConfig<'static> {
        LinkConfig::new(
            blinky_api_evolved::BLINKY_API_FULL_GID,
            blinky_api_evolved::BLINKY_API_WIRE_VERSION,
            server_impl::api_hash(),
            server_impl::image_facts(),
            ww_client_server::COMPACT_VERSION,
        )
    }

    impl WireWeaverAsyncApiBackend for Firmware {
        type Medium = ();

        async fn process_bytes<'a>(
            &mut self,
            out: &mut EventWriter<'_, impl MessageSink>,
            medium: (),
            data: &[u8],
            scratch: &'a mut [u8],
        ) -> Result<&'a [u8], shrink_wrap::Error> {
            self.process_request_bytes(data, scratch, out, medium).await
        }

        fn version(&self) -> FullVersion<'_> {
            blinky_api_evolved::BLINKY_API_FULL_GID
        }
    }

    mod server_impl {
        wire_weaver::ww_codegen!(
            blinky_api_evolved :: BlinkyApi for super::Firmware,
            server = true, no_alloc = true, use_async = true,
            method_model = "_=immediate",
            property_model = "_=get_set",
            introspect = "with_docs",
        );
    }

    impl Firmware {
        async fn led_on(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
            self.led = true;
            Ready(())
        }

        async fn led_off(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
            self.led = false;
            Ready(())
        }

        // --8<-- [start:toggle]
        async fn led_toggle(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
            self.led = !self.led;
            Ready(())
        }
        // --8<-- [end:toggle]
    }
}

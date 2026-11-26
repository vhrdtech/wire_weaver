//! The blinky firmware of both versions, simulated on a PC and served in process: [serve_w1_0] runs the firmware
//! already in the field (`blinky_api` 0.1.0 at w1.0, the `blinky` example's device), [serve_w1_1] the new one (0.1.1
//! at w1.1, with `led_toggle`). Each serves the WebSocket clients of a [WsPort], made with
//! `in_process::ws_device(path)` before a host connects to `path`.

use std::cell::Cell;
use std::future::Future;
use std::pin::pin;
use std::rc::Rc;

use blinky_api_evolved::Blinks;

use wire_weaver::prelude::*;
use wire_weaver::{MessageSink, WireWeaverAsyncApiBackend};
use wire_weaver_client::in_process::{TokioClock, WsPort};
use ww_device::LinkConfig;
use ww_device::ws::{IoSocket, WsBuffers, WsConnection, ws_server};

/// Longest message either side sends.
const MAX_MESSAGE_LEN: usize = 1024;

/// Serves firmware 0.1.0, the `blinky` example's device, to the clients of `port` until `stop` resolves; returns
/// whether the LED is on.
pub async fn serve_w1_0(port: WsPort, stop: impl Future<Output = ()>) -> bool {
    let on = Rc::new(Cell::new(false));
    let led: &'static mut LedState = Box::leak(Box::new(LedState(on.clone())));
    blinky::simulated(port, led, stop).await;
    on.get()
}

/// An LED whose state a test reads after the firmware stops.
struct LedState(Rc<Cell<bool>>);

impl blinky::Led for LedState {
    fn set(&mut self, on: bool) {
        self.0.set(on);
    }
}

/// Serves firmware 0.1.1 to the clients of `port` until `stop` resolves; returns whether the LED is on.
pub async fn serve_w1_1(port: WsPort, stop: impl Future<Output = ()>) -> bool {
    let mut firmware = w1_1::Firmware {
        led: false,
        blinks: Blinks::new(0),
    };
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

/// The new firmware: `blinky_api` 0.1.1, wire w1.1.
mod w1_1 {
    use super::*;

    pub(super) struct Firmware {
        pub(super) led: bool,
        pub(super) blinks: Blinks,
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
        /// Turns the LED on or off, counts a blink when it goes on, and returns whether it was on before.
        fn switch(&mut self, on: bool) -> bool {
            let was_on = self.led;
            if on && !was_on {
                self.blinks = Blinks::new(self.blinks.get().wrapping_add(1));
            }
            self.led = on;
            was_on
        }

        async fn led_on(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
            self.switch(true);
            Ready(())
        }

        async fn led_off(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
            self.switch(false);
            Ready(())
        }

        async fn set_led(&mut self, _cx: &mut Context<'_, impl EventOut>, on: bool) -> RpcResult<bool> {
            Ready(self.switch(on))
        }

        async fn blinks(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<Blinks> {
            Ready(self.blinks)
        }

        // --8<-- [start:toggle]
        async fn led_toggle(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
            let on = !self.led;
            self.switch(on);
            Ready(())
        }
        // --8<-- [end:toggle]
    }
}

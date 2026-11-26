//! The blinky example device ([blinky_api]): the server and its handlers, written once.
//!
//! The same code runs in three places. A simulated device on a PC serves it over WebSocket; the firmware of a board
//! serves it over USB; tests serve it in process. What differs between them is only the [Led] and the transport
//! given to [serve].
//!
//! ```text
//!              blinky_api (the trait)
//!                     |
//!          blinky_device (this crate: Blinky, serve)
//!             /                    \
//!   a printed LED over WebSocket    PA5 on the Nucleo G0B1RE over USB
//!   (examples/blinky)               (examples_mcu/nucleo_g0b1re)
//! ```
#![cfg_attr(not(feature = "std"), no_std)]

use core::future::Future;
use core::pin::pin;

use blinky_api::Blinks;
use embassy_futures::select::{Either, select};
use wire_weaver::prelude::*;
use wire_weaver::{MessageSink, WireWeaverAsyncApiBackend};
use ww_device::{Clock, LinkConfig, MessageRx, MessageTx, Server};

// --8<-- [start:led]
/// What a board gives the blinky device: one LED.
pub trait Led {
    /// Turns the LED on (`true`) or off (`false`).
    fn set(&mut self, on: bool);
}
// --8<-- [end:led]

// --8<-- [start:state]
/// The blinky device: the server state that the generated code dispatches to.
pub struct Blinky {
    led: &'static mut dyn Led,
    on: bool,
    blinks: Blinks,
}
// --8<-- [end:state]

impl Blinky {
    /// A blinky device on `led`, with the LED off.
    pub fn new(led: &'static mut dyn Led) -> Self {
        led.set(false);
        Blinky {
            led,
            on: false,
            blinks: Blinks::new(0),
        }
    }

    /// The link configuration a transport needs to serve this API.
    pub fn link_config() -> LinkConfig<'static> {
        LinkConfig::new(
            blinky_api::BLINKY_API_FULL_GID,
            blinky_api::BLINKY_API_WIRE_VERSION,
            api_impl::api_hash(),
            api_impl::image_facts(),
            ww_client_server::COMPACT_VERSION,
        )
    }

    /// The API id a USB device names itself by.
    pub const API_ID: &'static str = api_impl::API_ID;

    /// Turns the LED on or off, counts a blink when it goes on, and returns whether it was on before.
    fn switch(&mut self, on: bool) -> bool {
        let was_on = self.on;
        if on && !was_on {
            self.blinks = Blinks::new(self.blinks.get().wrapping_add(1));
        }
        self.on = on;
        self.led.set(on);
        was_on
    }
}

// --8<-- [start:handlers]
impl Blinky {
    async fn led_on(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
        self.switch(true);
        Ready(())
    }

    async fn led_off(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
        self.switch(false);
        Ready(())
    }
    // --8<-- [end:handlers]

    // --8<-- [start:method_handlers]
    async fn set_led(&mut self, _cx: &mut Context<'_, impl EventOut>, on: bool) -> RpcResult<bool> {
        Ready(self.switch(on))
    }

    async fn blinks(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<Blinks> {
        Ready(self.blinks)
    }
    // --8<-- [end:method_handlers]
}

// --8<-- [start:codegen]
mod api_impl {
    wire_weaver::ww_codegen!(
        blinky_api :: BlinkyApi for super::Blinky,
        server = true, no_alloc = true, use_async = true,
        method_model = "_=immediate",
        property_model = "_=get_set",
        introspect = "with_docs",
    );
}
// --8<-- [end:codegen]

// --8<-- [start:backend]
impl WireWeaverAsyncApiBackend for Blinky {
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
// --8<-- [end:backend]

// --8<-- [start:serve]
/// Serves `blinky` on `server` until `stop` resolves: the one event loop of the simulated device, the firmware and
/// the tests. A firmware that never stops passes [core::future::pending].
pub async fn serve<Tx: MessageTx, Rx: MessageRx, C: Clock>(
    server: &mut Server<'_, Tx, Rx, C>,
    blinky: &mut Blinky,
    stop: impl Future<Output = ()>,
) {
    let mut stop = pin!(stop);
    loop {
        let ready = match select(server.wait(), &mut stop).await {
            Either::First(ready) => ready,
            Either::Second(()) => return,
        };
        _ = server.handle(ready, blinky).await;
    }
}
// --8<-- [end:serve]

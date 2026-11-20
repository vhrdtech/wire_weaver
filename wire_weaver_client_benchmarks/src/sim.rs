//! Blinky devices in this process, over simulated USB (`in_process::usb_device`): the firmware's API and packet size,
//! each device on its own thread, as `tests_common` runs test devices.

use tests_common::{DeviceApi, TestDevice, TestProcessEventsAsync};
use wire_weaver::EventOut;
use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::Error as ShrinkWrapError;

/// The packet size of a high-speed USB bulk endpoint.
pub const HIGH_SPEED_PACKET_BYTES: usize = 512;

/// The blinky server, its LEDs gone.
struct Server;

mod server_impl {
    wire_weaver::ww_codegen!(
        blinky_api :: BlinkyApi for super::Server,
        server = true, no_alloc = true, use_async = true,
        method_model = "_=immediate",
        property_model = "_=get_set",
        introspect = "no_docs",
    );
}

impl Server {
    async fn led_on(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
        Ready(())
    }

    async fn led_off(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
        Ready(())
    }
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

/// Starts a blinky device at in-process `path` with USB packets of at most `packet_bytes`; it runs until dropped.
pub fn start(path: &str, packet_bytes: usize) -> TestDevice {
    tests_common::start_device_async_usb(
        path,
        packet_bytes,
        Server,
        blinky_api::BLINKY_API_FULL_GID,
        blinky_api::BLINKY_API_WIRE_VERSION,
        DeviceApi {
            hash: server_impl::api_hash(),
            // nothing seeded into a cache: the bench's client finds what a real host would
            ww_self_bytes: &[],
        },
    )
}

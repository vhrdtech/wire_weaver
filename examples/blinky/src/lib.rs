//! Client of the blinky example device ([blinky_api]), and the device simulated on a PC.
//!
//! [Blinky] is the host side: the generated calls `led_on()`, `led_off()`, `set_led()` and `blinks()`.
//! [simulated] runs the device itself, `blinky_device` on an [Led] of the caller's, over WebSocket: from a TCP
//! listener in the `blinky_sim` example, or in process for tests.
//!
//! ```text
//! terminal 1: cargo run -p blinky --example blinky_sim
//! terminal 2: cargo run -p blinky --example blinky_host
//! ```
use std::future::Future;

pub use blinky_device::Led;
use blinky_device::{Blinky as Device, serve};
use futures_util::Stream;
use futures_util::io::{AsyncRead, AsyncWrite};
pub use wire_weaver_client::ClientConfig;
use wire_weaver_client::in_process::TokioClock;
use wire_weaver_client::{Commander, DeviceApiInfo, WwClient};
use ww_device::ws::{IoSocket, WsBuffers, WsConnection, ws_server};

/// URL path the simulated device serves the API at.
pub const WS_PATH: &str = "/ww";

/// Longest message either side sends.
const MAX_MESSAGE_LEN: usize = 1024;

// --8<-- [start:client]
/// The host side of a blinky device: the calls `ww_codegen!` generates for [blinky_api::BlinkyApi] live on it.
pub struct Blinky {
    cmd: Commander,
}

impl WwClient for Blinky {
    /// A board over USB, by the VID:PID its firmware enumerates with.
    fn default_config() -> ClientConfig {
        ClientConfig::new().usb_vid_pid(0xc0de, 0xcafe)
    }

    fn try_from_cmd(cmd: Commander) -> Result<Self, wire_weaver_client::Error> {
        Ok(Self { cmd })
    }
}

mod api_client {
    wire_weaver::ww_codegen!(
        blinky_api :: BlinkyApi for crate::Blinky,
        client = "std_client",
    );
}
// --8<-- [end:client]

impl Blinky {
    /// What the device said about itself when it connected.
    pub fn info(&self) -> DeviceApiInfo {
        self.cmd.info()
    }
}

// --8<-- [start:simulated]
/// Serves a blinky device on `led` to the WebSocket clients `incoming` yields, one at a time, until `stop`
/// resolves. The device's futures are not `Send`: run it on a current-thread runtime or in a `LocalSet`.
pub async fn simulated<I, S>(incoming: I, led: &'static mut dyn Led, stop: impl Future<Output = ()>)
where
    I: Stream<Item = S> + Unpin,
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut device = Device::new(led);
    let conn = WsConnection::new(IoSocket::new(incoming));
    let mut buffers = Box::new(WsBuffers::<MAX_MESSAGE_LEN>::new());
    let mut server = ws_server(
        Device::link_config(),
        &conn,
        TokioClock::new(),
        &mut buffers,
    );
    serve(&mut server, &mut device, stop).await;
}
// --8<-- [end:simulated]

//! Client of the thermometer example device ([thermometer_api]), and the device simulated on a PC.
//!
//! [Thermometer] is the host side: the generated calls, `read_once()`, `write_mode()`, `observe_gain()`,
//! `samples()` and the rest. [simulated] runs the device itself, `thermometer_device` on its [SimBoard], over
//! WebSocket: from a TCP listener in the `thermometer_sim` example, or in process for tests.
//!
//! ```text
//! terminal 1: cargo run -p thermometer --example thermometer_sim
//! terminal 2: cargo run -p thermometer --example thermometer_read
//! ```
use std::future::Future;

use futures_util::Stream;
use futures_util::io::{AsyncRead, AsyncWrite};
use thermometer_device::Thermometer as Device;
pub use thermometer_device::{Counters, SimBoard};
pub use wire_weaver_client::ClientConfig;
use wire_weaver_client::in_process::TokioClock;
use wire_weaver_client::{Commander, DeviceApiInfo, WwClient};
use ww_device::ws::{IoSocket, WsBuffers, WsConnection, ws_server};

/// URL path the simulated device serves the API at.
pub const WS_PATH: &str = "/ww";

/// Longest message either side sends.
const MAX_MESSAGE_LEN: usize = 1024;

/// The host side of a thermometer: the calls `ww_codegen!` generates for [thermometer_api::Thermo] live on it.
pub struct Thermometer {
    cmd: Commander,
}

impl WwClient for Thermometer {
    fn default_config() -> ClientConfig {
        ClientConfig::new()
    }

    fn try_from_cmd(cmd: Commander) -> Result<Self, wire_weaver_client::Error> {
        Ok(Self { cmd })
    }
}

impl Thermometer {
    /// What the device said about itself when it connected.
    pub fn info(&self) -> DeviceApiInfo {
        self.cmd.info()
    }
}

mod api_client {
    wire_weaver::ww_codegen!(
        thermometer_api :: Thermo for crate::Thermometer,
        client = "std_client",
    );
}

// --8<-- [start:simulated]
/// Serves a thermometer on a [SimBoard] to the WebSocket clients `incoming` yields, one at a time, until `stop`
/// resolves; returns what it dropped or refused. The device's futures are not `Send`: run it on a current-thread
/// runtime or in a `LocalSet`.
pub async fn simulated<I, S>(incoming: I, stop: impl Future<Output = ()>) -> Counters
where
    I: Stream<Item = S> + Unpin,
    S: AsyncRead + AsyncWrite + Unpin,
{
    let board: &'static mut SimBoard = Box::leak(Box::default());
    let mut device = Device::new(board);
    let conn = WsConnection::new(IoSocket::new(incoming));
    let mut buffers = Box::new(WsBuffers::<MAX_MESSAGE_LEN>::new());
    let mut server = ws_server(
        Device::link_config(),
        &conn,
        TokioClock::new(),
        &mut buffers,
    );
    thermometer_device::serve(&mut server, &mut device, stop).await;
    device.counters()
}
// --8<-- [end:simulated]

//! Client of the bench supply example device ([bench_supply_api]), and the device simulated on a PC: [simulated]
//! serves it over WebSocket for [BenchSupply] to connect to; `bench_supply_hub_device` (`src/bin/hub_device.rs`)
//! hosts the same server and the ww hub in one process, for the pytest bench (EX-19, `examples/bench_supply_py`).
//!
//! Each channel loads its output with a fixed resistance (`LOAD_OHM`): `set_output(true)` refuses instead of
//! letting the load draw more than `current_limit`.
//!
//! ```text
//! terminal 1: cargo run -p bench_supply --example bench_supply_sim
//! terminal 2: cargo run -p bench_supply --example bench_supply_bench
//! ```
use std::future::Future;

use bench_supply_api::{Current, Measurement, OutputError, Voltage};
use embassy_futures::select::{Either, select};
use futures_util::Stream;
use futures_util::io::{AsyncRead, AsyncWrite};
use wire_weaver::prelude::*;
use wire_weaver::{MessageSink, WireWeaverAsyncApiBackend};
pub use wire_weaver_client::ClientConfig;
use wire_weaver_client::in_process::TokioClock;
use wire_weaver_client::{Commander, DeviceApiInfo, WwClient};
use ww_device::ws::{IoSocket, WsBuffers, WsConnection, ws_server};

/// URL path the simulated device serves the API at.
pub const WS_PATH: &str = "/ww";

/// Longest message either side sends.
const MAX_MESSAGE_LEN: usize = 1024;

/// Channels the simulated device has.
pub const CHANNELS: usize = 2;

/// Each channel's simulated load, in ohms: at `voltage_limit` it draws `voltage_limit / LOAD_OHM[channel]`.
const LOAD_OHM: [u32; CHANNELS] = [10, 2];

/// `bench_supply_api::Current`'s upper bound, in mA: what a computed draw is clamped to before it becomes one.
const CURRENT_MAX_MA: u32 = 5_000;

/// The host side of a bench supply: the calls `ww_codegen!` generates for [bench_supply_api::BenchSupply] live on
/// it.
pub struct BenchSupply {
    cmd: Commander,
}

impl WwClient for BenchSupply {
    fn default_config() -> ClientConfig {
        ClientConfig::new()
    }

    fn try_from_cmd(cmd: Commander) -> Result<Self, wire_weaver_client::Error> {
        Ok(Self { cmd })
    }
}

impl BenchSupply {
    /// What the device said about itself when it connected.
    pub fn info(&self) -> DeviceApiInfo {
        self.cmd.info()
    }
}

mod api_client {
    wire_weaver::ww_codegen!(
        bench_supply_api :: BenchSupply for crate::BenchSupply,
        client = "std_client",
    );
}

/// One channel's state: its limits, whether its output is on, and what it last measured.
#[derive(Debug, Clone, Copy)]
struct Channel {
    voltage_limit: Voltage,
    current_limit: Current,
    output_on: bool,
    measured: Measurement,
}

impl Default for Channel {
    fn default() -> Self {
        Channel {
            voltage_limit: Voltage::bounded(0),
            current_limit: Current::bounded(0),
            output_on: false,
            measured: Measurement {
                voltage: Voltage::bounded(0),
                current: Current::bounded(0),
            },
        }
    }
}

impl Channel {
    /// Current the load would draw at `voltage_limit`, were the output on.
    fn would_draw(&self, load_ohm: u32) -> Current {
        let ma = (u32::from(self.voltage_limit.get()) / load_ohm).min(CURRENT_MAX_MA);
        Current::bounded(ma as u16)
    }
}

/// The bench supply: the server state the generated code dispatches to.
pub struct Server {
    channels: [Channel; CHANNELS],
    observe: api_impl::ObserveState,
}

impl Server {
    /// A bench supply with every channel's output off and both limits at 0.
    pub fn new() -> Self {
        Server {
            channels: [Channel::default(); CHANNELS],
            observe: api_impl::ObserveState::new(Default::default()),
        }
    }
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

impl Server {
    fn valid_indices_root_channel(&mut self) -> ValidIndices<'_> {
        ValidIndices::range_u32(0..CHANNELS as u32)
    }

    /// The channel at a `channel[]` index the generated code has checked against
    /// [Self::valid_indices_root_channel].
    fn channel(index_chain: [ww_client_server::ResourceIndex; 1]) -> usize {
        index_chain[0].get().0 as usize
    }

    async fn set_channel_voltage_limit(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        index_chain: [ww_client_server::ResourceIndex; 1],
        value: Voltage,
    ) -> SetResult<CommonError<'_>> {
        self.channels[Self::channel(index_chain)].voltage_limit = value;
        Set
    }

    async fn get_channel_voltage_limit(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        index_chain: [ww_client_server::ResourceIndex; 1],
    ) -> GetResult<Voltage, CommonError<'_>> {
        Value(self.channels[Self::channel(index_chain)].voltage_limit)
    }

    async fn set_channel_current_limit(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        index_chain: [ww_client_server::ResourceIndex; 1],
        value: Current,
    ) -> SetResult<CommonError<'_>> {
        self.channels[Self::channel(index_chain)].current_limit = value;
        Set
    }

    async fn get_channel_current_limit(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        index_chain: [ww_client_server::ResourceIndex; 1],
    ) -> GetResult<Current, CommonError<'_>> {
        Value(self.channels[Self::channel(index_chain)].current_limit)
    }

    async fn get_channel_measured(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        index_chain: [ww_client_server::ResourceIndex; 1],
    ) -> GetResult<Measurement, CommonError<'_>> {
        Value(self.channels[Self::channel(index_chain)].measured)
    }

    async fn channel_set_output(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        index_chain: [ww_client_server::ResourceIndex; 1],
        on: bool,
    ) -> RpcResult<Result<(), wire_weaver::Error<'_, OutputError>>> {
        let i = Self::channel(index_chain);
        if !on {
            self.channels[i].output_on = false;
            self.channels[i].measured = Measurement {
                voltage: Voltage::bounded(0),
                current: Current::bounded(0),
            };
            self.mark_changed_channel_output_on(index_chain);
            self.mark_changed_channel_measured(index_chain);
            return Ready(Ok(()));
        }
        let channel = self.channels[i];
        if channel.voltage_limit.get() == 0 {
            return Ready(Err(OutputError::NoVoltageSet.into()));
        }
        let would_draw = channel.would_draw(LOAD_OHM[i]);
        if would_draw.get() > channel.current_limit.get() {
            return Ready(Err(OutputError::OverCurrent { would_draw }.into()));
        }
        self.channels[i].output_on = true;
        self.channels[i].measured = Measurement {
            voltage: channel.voltage_limit,
            current: would_draw,
        };
        self.mark_changed_channel_output_on(index_chain);
        self.mark_changed_channel_measured(index_chain);
        Ready(Ok(()))
    }

    async fn get_channel_output_on(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        index_chain: [ww_client_server::ResourceIndex; 1],
    ) -> GetResult<bool, CommonError<'_>> {
        Value(self.channels[Self::channel(index_chain)].output_on)
    }

    fn observe_state(&mut self) -> &mut api_impl::ObserveState {
        &mut self.observe
    }
}

pub mod api_impl {
    wire_weaver::ww_codegen!(
        bench_supply_api :: BenchSupply for super::Server,
        server = true, no_alloc = true, use_async = true,
        method_model = "_=immediate",
        property_model = "_=get_set",
        introspect = "with_docs",
    );
}

impl WireWeaverAsyncApiBackend for Server {
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
        bench_supply_api::BENCH_SUPPLY_FULL_GID
    }
}

/// The link configuration a transport needs to serve [Server].
pub fn link_config() -> ww_device::LinkConfig<'static> {
    ww_device::LinkConfig::new(
        bench_supply_api::BENCH_SUPPLY_FULL_GID,
        bench_supply_api::BENCH_SUPPLY_WIRE_VERSION,
        api_impl::api_hash(),
        api_impl::image_facts(),
        ww_client_server::COMPACT_VERSION,
    )
}

/// Device key `bench_supply_hub_device` registers [Server] under, and every client connects to through the ww hub
/// (`ClientConfig::hub_device`, Python's `connect(hub_device=...)`), for the pytest bench (`examples/bench_supply_py`).
pub const DEVICE_KEY: &str = "inproc:bench-supply";
/// [DEVICE_KEY] without its `inproc:` scheme, what `tests_common::start_device_async` takes.
pub const DEVICE_PATH: &str = "bench-supply";

impl tests_common::TestProcessEventsAsync for Server {
    type Medium = ();

    async fn process_request_bytes<'a>(
        &mut self,
        bytes: &[u8],
        scratch: &'a mut [u8],
        out: &mut impl EventOut,
        medium: (),
    ) -> Result<&'a [u8], shrink_wrap::Error> {
        self.process_request_bytes(bytes, scratch, out, medium)
            .await
    }
}

// --8<-- [start:simulated]
/// Serves a bench supply to the WebSocket clients `incoming` yields, one at a time, until `stop` resolves. The
/// device's futures are not `Send`: run it on a current-thread runtime or in a `LocalSet`.
pub async fn simulated<I, S>(incoming: I, stop: impl Future<Output = ()>)
where
    I: Stream<Item = S> + Unpin,
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut server = Server::new();
    let conn = WsConnection::new(IoSocket::new(incoming));
    let mut buffers = Box::new(WsBuffers::<MAX_MESSAGE_LEN>::new());
    let mut ww_server = ws_server(link_config(), &conn, TokioClock::new(), &mut buffers);
    let mut stop = core::pin::pin!(stop);
    loop {
        let ready = match select(ww_server.wait(), &mut stop).await {
            Either::First(ready) => ready,
            Either::Second(()) => return,
        };
        ww_server.handle(ready, &mut server).await;
        let mut buf = [0u8; 64];
        let now_us = ww_server.now().as_micros();
        _ = server
            .poll_properties(&mut ww_server.sink(), (), now_us, &mut buf)
            .await;
    }
}
// --8<-- [end:simulated]

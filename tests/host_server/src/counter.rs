//! A second API for the byte-stream tests (TR-19): `keep_trying_api::Counter`, served next to `HostServer` on one
//! listener and alone on stdio by the `stdio_server` binary.

use wire_weaver::prelude::*;
use wire_weaver_server::Connection;
use ww_si::Fixed;
use ww_si::prefix::One;
use ww_si::units::Item;

/// What `ticks` reads on every connection.
pub const TICKS: u32 = 42;

/// The longest message the counter takes.
pub const MESSAGE_LEN_MAX: usize = 4096;

/// The server of one connection.
pub struct Counter {
    observe: api_impl::ObserveState,
}

impl Counter {
    /// The backend of `connection`.
    pub fn make(_connection: &Connection) -> Self {
        Counter {
            observe: api_impl::ObserveState::new(Default::default()),
        }
    }

    fn observe_state(&mut self) -> &mut api_impl::ObserveState {
        &mut self.observe
    }

    async fn get_ticks(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<Fixed<Item, One, u32>, CommonError<'_>> {
        GetResult::Value(Fixed::new(TICKS))
    }
}

/// The link of the counter.
pub fn link() -> ww_device::LinkConfig<'static> {
    ww_device::LinkConfig::new(
        keep_trying_api::COUNTER_FULL_GID,
        keep_trying_api::COUNTER_WIRE_VERSION,
        api_impl::api_hash(),
        api_impl::image_facts(),
        ww_client_server::COMPACT_VERSION,
    )
}

pub mod api_impl {
    wire_weaver::ww_codegen!(
        keep_trying_api :: Counter for super::Counter,
        server = true, no_alloc = false, use_async = true,
        method_model = "_=immediate",
        property_model = "_=get_set",
    );
}

impl wire_weaver_server::Backend for Counter {}

impl WireWeaverAsyncApiBackend for Counter {
    type Medium = ();

    async fn process_bytes<'a>(
        &mut self,
        out: &mut EventWriter<'_, impl MessageSink>,
        medium: (),
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> Result<&'a [u8], ShrinkWrapError> {
        self.process_request_bytes(data, scratch, out, medium).await
    }

    fn version(&self) -> FullVersion<'_> {
        keep_trying_api::COUNTER_FULL_GID
    }
}

impl wire_weaver::WireWeaverSharedApiBackend for Counter {
    async fn process_exclusive_bytes<'a>(
        &mut self,
        origin: &wire_weaver::CallContext,
        out: &mut EventWriter<'_, impl MessageSink>,
        medium: (),
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> Result<&'a [u8], ShrinkWrapError> {
        self.process_request_from(origin, data, scratch, out, medium)
            .await
    }

    fn is_shared(data: &[u8]) -> bool {
        Counter::is_shared(data)
    }

    async fn process_shared_bytes<'a>(
        &self,
        origin: &wire_weaver::CallContext,
        out: &mut EventWriter<'_, impl MessageSink>,
        medium: (),
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> Result<&'a [u8], ShrinkWrapError> {
        self.process_shared_from(origin, data, scratch, out, medium)
            .await
    }
}

/// A client of the counter.
pub struct CounterClient {
    pub cmd: wire_weaver_client::Commander,
}

impl wire_weaver_client::WwClient for CounterClient {
    fn default_config() -> wire_weaver_client::ClientConfig {
        wire_weaver_client::ClientConfig::default()
    }

    fn try_from_cmd(cmd: wire_weaver_client::Commander) -> Result<Self, wire_weaver_client::Error> {
        Ok(Self { cmd })
    }
}

pub mod api_client {
    wire_weaver::ww_codegen!(
        keep_trying_api :: Counter for super::CounterClient,
        client = "std_client",
    );
}

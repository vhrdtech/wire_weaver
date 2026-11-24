//! The modules a test loads into the TV's hub slot.

use std::sync::{Arc, Mutex};

use wire_weaver::prelude::*;
use wire_weaver::ww_api::mount::{MountName, MountPresence};
use ww_client_server::ResourceIndex;
use ww_client_server::mount::{MountAnswer, MountRequest};
use ww_device::mount::{Linked, Module, ModuleError, Outbox};

/// The models the modules hand their slot: the API-73 bundles of their roots.
pub mod models {
    pub mod hub {
        wire_weaver::ww_codegen!(mount_backend_api::Hub, bundle = "no_docs");
    }
    pub mod tv {
        wire_weaver::ww_codegen!(mounts_api::Tv, bundle = "no_docs");
    }
}

/// What a test tells the hub module, ticks to send on its open stream, and what the module tells back.
#[derive(Default)]
pub struct HubControl {
    pub ticks: Vec<u8>,
    /// Requests of `hang` the module holds.
    pub hung: usize,
}

/// The hub of `mount_backend_api` as a module: a blocking `no_alloc` server whose poll sends the ticks the test
/// queued while its stream is open.
pub struct HubModule {
    pub control: Arc<Mutex<HubControl>>,
    pub lamp_on: bool,
    pub ticking: bool,
    pub observe: hub_impl::ObserveState,
}

impl HubModule {
    pub fn new(control: Arc<Mutex<HubControl>>) -> Self {
        HubModule {
            control,
            lamp_on: false,
            ticking: false,
            observe: hub_impl::ObserveState::new(Default::default()),
        }
    }

    fn add(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        a: u32,
        b: u32,
    ) -> RpcResult<u32> {
        Ready(a.wrapping_add(b))
    }

    fn hang(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
        self.control.lock().unwrap().hung += 1;
        Deferred
    }

    fn ticks(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
        self.ticking = true;
        Ready(())
    }

    fn close_ticks(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {
        self.ticking = false;
    }

    fn sideband_ticks(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        sideband: ww_client_server::StreamSideband,
    ) -> Option<ww_client_server::StreamSideband> {
        Some(sideband)
    }

    fn stray(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>, _seq: u32) -> RpcResult<()> {
        Deferred
    }

    fn lamp_toggle(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<bool> {
        self.lamp_on = !self.lamp_on;
        Ready(self.lamp_on)
    }

    fn get_lamp_on(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> GetResult<bool, CommonError<'_>> {
        Value(self.lamp_on)
    }

    fn observe_state(&mut self) -> &mut hub_impl::ObserveState {
        &mut self.observe
    }
}

pub mod hub_impl {
    wire_weaver::ww_codegen!(
        mount_backend_api :: Hub for super::HubModule,
        server = true, no_alloc = true, use_async = false,
        method_model = "hang=deferred, stray=deferred, _=immediate",
        property_model = "_=get_set",
        introspect = "no_docs",
        debug_to_file = "../../target/tests_mount_module_hub.rs"
    );
}

impl Module for HubModule {
    fn model(&self) -> &[u8] {
        &hub_impl::WW_SELF_BYTES_NO_DOCS
    }

    fn process<'r>(
        &mut self,
        request: &[u8],
        events: &mut Outbox<'_>,
        reply: &'r mut [u8],
    ) -> Result<&'r [u8], ModuleError> {
        self.process_request_bytes(request, reply, events, ())
            .map_err(|_| ModuleError::ReplyTooLong)
    }

    fn poll(&mut self, events: &mut Outbox<'_>) -> Result<(), ModuleError> {
        let ticks = std::mem::take(&mut self.control.lock().unwrap().ticks);
        if self.ticking {
            for tick in ticks {
                _ = hub_impl::stream_data_ser().ticks_send_blocking(&[tick], events);
            }
        }
        Ok(())
    }
}

/// A second API for the slot: a TV with nothing mounted, linked as is.
pub struct TvModule {
    pub valid: Vec<UNib32>,
    pub observe: tv_impl::ObserveState,
}

impl TvModule {
    pub fn linked() -> Linked<TvModule> {
        Linked {
            server: TvModule {
                valid: vec![],
                observe: tv_impl::ObserveState::new(Default::default()),
            },
            model: &tv_impl::WW_SELF_BYTES_NO_DOCS,
        }
    }

    fn get_hub_presence(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> GetResult<MountPresence<'_>, CommonError<'_>> {
        Value(MountPresence::Absent)
    }

    fn hub_request(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _request: MountRequest<'_, '_>,
    ) -> MountAnswer {
        MountAnswer::Unavailable
    }

    fn valid_indices_root_ext(&mut self) -> ValidIndices<'_> {
        ValidIndices::List(RefVec::Slice { slice: &self.valid })
    }

    fn get_ext_presence(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _index_chain: [ResourceIndex; 1],
    ) -> GetResult<MountPresence<'_>, CommonError<'_>> {
        Value(MountPresence::Absent)
    }

    fn ext_request(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _index_chain: [ResourceIndex; 1],
        _request: MountRequest<'_, '_>,
    ) -> MountAnswer {
        MountAnswer::Unavailable
    }

    fn ext_index_of(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _key: MountName<'_>,
    ) -> RpcResult<Option<UNib32>> {
        Ready(None)
    }

    fn get_ext_key(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _index_chain: [ResourceIndex; 1],
    ) -> GetResult<MountName<'_>, CommonError<'_>> {
        Value(MountName(MaxLen::new("").expect("empty fits")))
    }

    fn mute(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
        Ready(())
    }

    fn observe_state(&mut self) -> &mut tv_impl::ObserveState {
        &mut self.observe
    }
}

pub mod tv_impl {
    wire_weaver::ww_codegen!(
        mounts_api :: Tv for super::TvModule,
        server = true, no_alloc = true, use_async = false,
        method_model = "_=immediate",
        property_model = "_=get_set",
        introspect = "no_docs"
    );
}

impl wire_weaver::WireWeaverApiBackend for TvModule {
    type Medium = ();

    fn process_bytes<'a>(
        &mut self,
        out: &mut impl BlockingEventOut,
        medium: (),
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> Result<&'a [u8], ShrinkWrapError> {
        self.process_request_bytes(data, scratch, out, medium)
    }

    fn version(&self) -> FullVersion<'_> {
        mounts_api::TV_FULL_GID
    }
}

/// A module that misbehaves: garbage and a reply under a seq it wasn't sent with every request, then an answer that
/// doesn't decode; it fails at the request at `[0]` (the hub's `add`).
pub struct Broken;

impl Module for Broken {
    fn model(&self) -> &[u8] {
        &hub_impl::WW_SELF_BYTES_NO_DOCS
    }

    fn process<'r>(
        &mut self,
        request: &[u8],
        events: &mut Outbox<'_>,
        reply: &'r mut [u8],
    ) -> Result<&'r [u8], ModuleError> {
        let request =
            ww_client_server::Request::from_ww_bytes(request).map_err(|_| ModuleError::Fault)?;
        let ww_client_server::PathKind::Absolute { path } = &request.path_kind else {
            return Err(ModuleError::Fault);
        };
        if path.iter().map(|id| id.map(|id| id.get().0)).eq([Ok(0)]) {
            return Err(ModuleError::Fault);
        }
        _ = events.push(&[0xff; 7]);
        let mut stray = [0u8; 16];
        if let Ok(event) =
            ww_client_server::util::ser_value_event(&mut stray, request.seq.0.wrapping_add(1), &[1])
        {
            _ = events.push(event);
        }
        let garbage = reply.get_mut(..3).ok_or(ModuleError::ReplyTooLong)?;
        garbage.copy_from_slice(&[0xff, 0xff, 0xff]);
        Ok(garbage)
    }
}

/// Every module of the tests, in one slot type.
pub enum Loaded {
    Hub(HubModule),
    Tv(Box<Linked<TvModule>>),
    Broken(Broken),
}

impl Module for Loaded {
    fn model(&self) -> &[u8] {
        match self {
            Loaded::Hub(m) => m.model(),
            Loaded::Tv(m) => m.model(),
            Loaded::Broken(m) => m.model(),
        }
    }

    fn process<'r>(
        &mut self,
        request: &[u8],
        events: &mut Outbox<'_>,
        reply: &'r mut [u8],
    ) -> Result<&'r [u8], ModuleError> {
        match self {
            Loaded::Hub(m) => m.process(request, events, reply),
            Loaded::Tv(m) => m.process(request, events, reply),
            Loaded::Broken(m) => m.process(request, events, reply),
        }
    }

    fn poll(&mut self, events: &mut Outbox<'_>) -> Result<(), ModuleError> {
        match self {
            Loaded::Hub(m) => m.poll(events),
            Loaded::Tv(m) => m.poll(events),
            Loaded::Broken(m) => m.poll(events),
        }
    }
}

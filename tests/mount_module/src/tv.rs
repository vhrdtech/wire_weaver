//! The parent: the TV of `mounts_api`, an async `no_alloc` server whose hub slot is a [ModuleSlot]. The test loads
//! and unloads modules through [Control]; the TV applies it at its next poll, as a firmware task would.

use std::sync::{Arc, Mutex};

use tests_common::{TestDevice, TestProcessEventsAsync};
use wire_weaver::prelude::*;
use wire_weaver::ww_api::mount::{MountName, MountPresence};
use ww_client_server::ResourceIndex;
use ww_client_server::mount::{MountAnswer, MountRequest};
use ww_device::mount::{ModuleCounters, ModuleSlot};

use crate::modules::Loaded;

/// The front link's message limit in the tests, and the bytes of a module's events per turn.
pub const MSG: usize = 512;
pub const OUT: usize = 1024;

/// What the test asks of the TV's hub slot, and what the slot reports back.
#[derive(Default)]
pub struct Control {
    /// Loaded at the next poll; `Some(None)` unloads.
    pub next: Option<Option<Loaded>>,
    /// The slot's counters after its last request or poll.
    pub counters: ModuleCounters,
}

pub struct Tv {
    pub hub: Box<ModuleSlot<Loaded, MSG, OUT>>,
    pub control: Arc<Mutex<Control>>,
    pub valid: Vec<UNib32>,
    pub observe: api_impl::ObserveState,
    pub started: std::time::Instant,
}

impl Tv {
    async fn get_hub_presence(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<MountPresence<'_>, CommonError<'_>> {
        Value(self.hub.presence())
    }

    async fn hub_request(
        &mut self,
        cx: &mut Context<'_, impl EventOut>,
        request: MountRequest<'_, '_>,
    ) -> MountAnswer {
        let answer = self.hub.forward(cx, &request).await;
        self.control.lock().unwrap().counters = self.hub.counters();
        answer
    }

    fn valid_indices_root_ext(&mut self) -> ValidIndices<'_> {
        ValidIndices::List(RefVec::Slice { slice: &self.valid })
    }

    async fn get_ext_presence(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        _index_chain: [ResourceIndex; 1],
    ) -> GetResult<MountPresence<'_>, CommonError<'_>> {
        Value(MountPresence::Absent)
    }

    async fn ext_request(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        _index_chain: [ResourceIndex; 1],
        _request: MountRequest<'_, '_>,
    ) -> MountAnswer {
        MountAnswer::Unavailable
    }

    async fn ext_index_of(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        _key: MountName<'_>,
    ) -> RpcResult<Option<UNib32>> {
        Ready(None)
    }

    async fn get_ext_key(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        _index_chain: [ResourceIndex; 1],
    ) -> GetResult<MountName<'_>, CommonError<'_>> {
        Value(MountName(MaxLen::new("").expect("empty fits")))
    }

    async fn mute(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
        Ready(())
    }

    fn observe_state(&mut self) -> &mut api_impl::ObserveState {
        &mut self.observe
    }
}

pub mod api_impl {
    wire_weaver::ww_codegen!(
        mounts_api :: Tv for super::Tv,
        server = true, no_alloc = true, use_async = true,
        method_model = "_=immediate",
        property_model = "_=get_set",
        introspect = "no_docs",
        debug_to_file = "../../target/tests_mount_module_tv.rs"
    );
}

impl TestProcessEventsAsync for Tv {
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

    async fn poll(&mut self, out: &mut impl EventOut, _now_ms: u64) {
        let next = self.control.lock().unwrap().next.take();
        match next {
            Some(Some(module)) => {
                if let Err((error, _)) = self.hub.load(module) {
                    panic!("the test's modules load: {error:?}");
                }
            }
            Some(None) => {
                self.hub.unload();
            }
            None => {}
        }
        self.hub.poll(out).await;
        if self.hub.take_changed() {
            self.mark_changed_hub_presence();
        }
        self.control.lock().unwrap().counters = self.hub.counters();
        let now_us = self.started.elapsed().as_micros() as u64;
        let mut buf = [0u8; 512];
        self.poll_properties(out, (), now_us, &mut buf)
            .await
            .expect("updates sent");
    }
}

/// The TV on the in-process link at `path`, its hub slot empty, `ext` with the one empty slot 0.
pub fn start(path: &str, control: Arc<Mutex<Control>>) -> TestDevice {
    let _ = tracing_subscriber::fmt::try_init();
    let tv = Tv {
        hub: Box::new(ModuleSlot::new()),
        control,
        valid: vec![UNib32(0)],
        observe: api_impl::ObserveState::new(Default::default()),
        started: std::time::Instant::now(),
    };
    tests_common::start_device_async(
        path,
        tv,
        mounts_api::TV_FULL_GID,
        mounts_api::TV_WIRE_VERSION,
        tests_common::api!(api_impl),
    )
}

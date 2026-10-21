//! Hosts the ww hub and one in-process test device (`PytestDevice`, a single `counter` property) in the same
//! process, for `wire_weaver_py`'s pytest hub tests (TOOL-18): no real hardware needed. Two or more Python
//! processes reach the same device as `inproc:pytest-device` through the hub's tunnel (SRV-29).
//!
//! Run directly by a pytest fixture (`WW_RUNTIME_DIR` set to a private temp dir, killed at teardown), not through
//! CLT-39's self-hosting: every client connects with `connect(hub_device=DEVICE_KEY)`, never starting a hub of
//! its own.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use tests_common::TestProcessEvents;
use wire_weaver::prelude::*;

/// Device key every client connects to through the hub (`ClientConfig::hub_device`).
pub const DEVICE_KEY: &str = "inproc:pytest-device";
const DEVICE_PATH: &str = "pytest-device";

struct Server {
    counter: Arc<AtomicU32>,
}

impl Server {
    fn get_counter(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> GetResult<u32, ()> {
        Value(self.counter.load(Ordering::SeqCst))
    }

    fn set_counter(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        value: u32,
    ) -> SetResult<()> {
        self.counter.store(value, Ordering::SeqCst);
        Set
    }
}

mod api_impl {
    wire_weaver::ww_codegen!(
        hub_pytest_device_api :: PytestDevice for super::Server,
        server = true, no_alloc = true, use_async = false,
        method_model = "_=immediate",
        property_model = "_=get_set",
    );
}

impl TestProcessEvents for Server {
    type Medium = ();

    fn process_request_bytes<'a>(
        &mut self,
        bytes: &[u8],
        scratch: &'a mut [u8],
        out: &mut impl BlockingEventOut,
        medium: (),
    ) -> Result<&'a [u8], ShrinkWrapError> {
        self.process_request_bytes(bytes, scratch, out, medium)
    }
}

fn main() {
    let counter = Arc::new(AtomicU32::new(0));
    // registered in this process, reached by the hub as `inproc:pytest-device` (ww_hub's device.rs config_for)
    let _device = tests_common::start_device(
        DEVICE_PATH,
        Server { counter },
        hub_pytest_device_api::PYTEST_DEVICE_FULL_GID,
        api_impl::api_hash(),
    );

    let config = ww_hub::HubConfig::from_env().expect("hub config from the environment");
    eprintln!(
        "hub_pytest_device: runtime dir {}, device {DEVICE_KEY}",
        config.runtime_dir.display()
    );
    match ww_hub::run_blocking_with(config) {
        Ok(exit) => eprintln!("hub_pytest_device: hub exited: {exit:?}"),
        Err(e) => {
            eprintln!("hub_pytest_device: {e}");
            std::process::exit(1);
        }
    }
}

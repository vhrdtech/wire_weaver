//! Test API root for `hub_pytest_device` (TOOL-18): one shared `counter` property, read and written by Python
//! processes through the ww hub without real hardware (`inproc:` device, `docs/transport/hub.md`).

use ww_api::prelude::*;

#[ww_api_root]
#[effect(edit)]
pub trait PytestDevice {
    /// Shared state: one process writes it, another reads it, both through the ww hub's tunnel.
    property!(rw counter: u32);
}

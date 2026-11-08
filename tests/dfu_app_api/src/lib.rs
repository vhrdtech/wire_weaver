//! The app of an A/B or stored-plan device: receives updates itself, every update trait. Used by the `ww_dfu` scenario tests (`tests/dfu_scenarios`).
#![no_std]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait DfuApp {
    ww_impl!(node: ww_dfu::UpdateNode);
    ww_impl!(boot: ww_dfu::BootControl);
    ww_impl!(inventory: ww_dfu::Inventory);
    ww_impl!(stage: ww_dfu::Stage);
    ww_impl!(bulk: ww_dfu::StageBulk);
}

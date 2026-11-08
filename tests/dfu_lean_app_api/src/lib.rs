//! The app of a device that updates in place live: only says who it is and reboots into its updater. Used by the `ww_dfu` scenario tests (`tests/dfu_scenarios`).
#![no_std]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait DfuLeanApp {
    ww_impl!(node: ww_dfu::UpdateNode);
    ww_impl!(boot: ww_dfu::BootControl);
}

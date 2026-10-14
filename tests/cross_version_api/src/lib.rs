//! The new version 0.4.0 of the API in `tests/cross_version_old_api`: the tree changed shape (wire-incompatible), so a
//! client generated from it can't connect to a device running 0.3.0, but its global trait calls still can.
#![no_std]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait CrossVersion {
    fn identify();
    ww_impl!(device: Device);
}

/// Everything about the unit that is not an interface
#[ww_trait]
pub trait Device {
    ww_impl!(update: ww_dfu::UpdateNode);
    ww_impl!(boot: ww_dfu::BootControl);
    fn uptime_ms() -> u64;
}

//! The new version 0.4.0 of the API in `tests/cross_version_old_api`: the tree changed shape (wire-incompatible, wire
//! line `w2.0` after `w1.0`), so a client generated from it can't connect to a device running 0.3.0, but its global
//! trait calls still can.
#![no_std]

use ww_api::prelude::*;

#[ww_api_root]
#[effect(edit)]
pub trait CrossVersion {
    fn identify();
    ww_impl!(device: Device);
}

/// Everything about the unit that is not an interface
#[ww_trait]
#[effect(edit)]
pub trait Device {
    ww_impl!(update: Node);
    ww_impl!(boot: global_boot_api::Boot);
    fn uptime_ms() -> u64;
}

/// Stands in for a real update-identity trait: only its presence (and so the API's shape) matters here.
#[ww_trait]
#[effect(edit)]
pub trait Node {
    fn node() -> u8;
}

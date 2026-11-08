//! An API at its old version 0.3.0, as firmware already in the field runs it. Used by `tests/cross_version`: a host
//! built against the new version (`cross_version_api` 0.4.0) still reboots it into its updater through global trait
//! paths.
//!
//! In a real project both are the same crate at two versions; a workspace can't hold two versions of one crate, so
//! the old one is kept here under its own name, and the test device reports it as `cross_version_api` 0.3.0.
#![no_std]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait CrossVersion {
    fn led_on();
    fn led_off();
    ww_impl!(device: Device);
}

/// Everything about the unit that is not an interface
#[ww_trait]
pub trait Device {
    fn uptime_s() -> u32;
    ww_impl!(boot: ww_dfu::BootControl);
}

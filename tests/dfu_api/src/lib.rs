//! A device that offers every `ww_dfu` trait, to check they go through codegen and work end to end.
#![no_std]

use ww_api::prelude::*;

#[ww_api_root]
pub trait DfuDevice {
    ww_impl!(node: ww_dfu::UpdateNode);
    ww_impl!(boot: ww_dfu::BootControl);
    ww_impl!(inventory: ww_dfu::Inventory);
    ww_impl!(stage: ww_dfu::Stage);
    ww_impl!(bulk: ww_dfu::StageBulk);
}

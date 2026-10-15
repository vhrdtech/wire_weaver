//! Test API root for the `ww_can_bus` group, time-triggered and CAN XL traits, used by `tests/io_can_group`.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait IoCanGroupTest {
    ww_impl!(can: ww_can_bus::CanGroup);
    ww_impl!(can_tt[]: ww_can_bus::CanTimeTriggered);
    ww_impl!(can_xl[]: ww_can_bus::CanXl);
}

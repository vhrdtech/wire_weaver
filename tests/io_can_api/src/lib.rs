//! Test API root for `ww_can_bus`, used by `tests/io_can`.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait IoCanTest {
    ww_impl!(can: ww_can_bus::Can);
}

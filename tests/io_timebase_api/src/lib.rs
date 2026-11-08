//! Test API root for `ww_timebase`: every trait of the crate at one level, used by `tests/io_timebase`.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait IoTimebaseTest {
    ww_impl!(clock: ww_timebase::Clock);
}

//! Test API root for `ww_counters`, used by `tests/counters`.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root(wire = "w1.0")]
pub trait CountersTest {
    ww_impl!(counters: ww_counters::Counters);
}

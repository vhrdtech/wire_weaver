//! Test API root for `ww_indication`, used by `tests/indication`.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait IndicationTest {
    ww_impl!(indication: ww_indication::Indication);
}

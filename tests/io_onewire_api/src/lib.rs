//! Test API root for `ww_onewire`: every trait of the crate at one level, used by `tests/io_onewire`.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait IoOneWireTest {
    ww_impl!(onewire: ww_onewire::OneWire);
}

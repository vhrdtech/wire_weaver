//! Test API root for `ww_gpio`: every trait of the crate at one level, used by `tests/io_gpio`.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait IoGpioTest {
    ww_impl!(bank[]: ww_gpio::Bank);
}

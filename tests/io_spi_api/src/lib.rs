//! Test API root for `ww_spi`: every trait of the crate at one level, used by `tests/io_spi`.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait IoSpiTest {
    ww_impl!(spi: ww_spi::Spi);
}

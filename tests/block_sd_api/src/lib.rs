//! Test API root for `ww_block` and `ww_sd`: a card served as a block device and as a raw SD card, the way a
//! bring-up firmware serves both, used by `tests/block_sd`.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait BlockSdTest {
    ww_impl!(card: ww_block::BlockDevice);
    ww_impl!(sd: ww_sd::SdCard);
}

/// A small firmware's card: only the block device, its `bulk` group left out (`omit = ["card.bulk"]`).
#[ww_api_root]
pub trait BlockLeanTest {
    ww_impl!(card: ww_block::BlockDevice);
}

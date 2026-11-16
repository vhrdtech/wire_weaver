//! Test API root for `ww_board_info`, used by `tests/board_info`.
#![cfg_attr(not(feature = "std"), no_std)]

use ww_api::prelude::*;

#[ww_api_root]
pub trait BoardInfoTest {
    ww_impl!(board: ww_board_info::BoardInfo);
}

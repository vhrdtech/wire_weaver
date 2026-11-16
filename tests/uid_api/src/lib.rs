//! Test API root for `ww_uid`, used by `tests/uid`.
#![cfg_attr(not(feature = "std"), no_std)]

use ww_api::prelude::*;

#[ww_api_root]
pub trait UidTest {
    ww_impl!(uid: ww_uid::Uid);
}

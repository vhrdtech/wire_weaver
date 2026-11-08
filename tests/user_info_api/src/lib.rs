//! Test API root for `ww_user_info`, used by `tests/user_info`.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait UserInfoTest {
    ww_impl!(user: ww_user_info::UserInfo);
}

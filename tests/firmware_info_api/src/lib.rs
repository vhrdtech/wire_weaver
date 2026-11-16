//! Test API root for `ww_firmware_info`: both traits at one level, used by `tests/firmware_info`.
#![cfg_attr(not(feature = "std"), no_std)]

use ww_api::prelude::*;

#[ww_api_root]
pub trait FirmwareInfoTest {
    ww_impl!(firmware: ww_firmware_info::FirmwareInfo);
    ww_impl!(reset: ww_firmware_info::LastReset);
}

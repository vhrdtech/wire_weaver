//! Test API root for `ww info` / `ww report`: every device contract trait, and methods that make the device log.
//! Used by `tests/device_report`.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait ReportTest {
    ww_impl!(firmware: ww_firmware_info::FirmwareInfo);
    ww_impl!(reset: ww_firmware_info::LastReset);
    ww_impl!(board: ww_board_info::BoardInfo);
    ww_impl!(uid: ww_uid::Uid);
    ww_impl!(user: ww_user_info::UserInfo);
    ww_impl!(defmt: ww_log::LogDefmt);
    ww_impl!(counters: ww_counters::Counters);

    /// Logs `count` defmt frames "sample {=u32}" from `first`, then pumps them into the store.
    fn log_samples(first: u32, count: u32);
}

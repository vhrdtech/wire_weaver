//! Test API root for `ww_log`: both traits, and methods that make the device log, used by `tests/log_defmt`.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
use ww_log::Severity;

#[ww_api_root]
pub trait LogTest {
    ww_impl!(defmt: ww_log::LogDefmt);
    ww_impl!(text: ww_log::LogText);

    /// Logs `count` defmt frames "sample {=u32}" with values from `first` (timestamp `10 * value`), then pumps
    /// them into the store.
    fn log_samples(first: u32, count: u32);
    /// Logs `count` frames "burst {=u32}" from `first` without pumping in between, then pumps: what doesn't fit the
    /// logger's ring is dropped and reported as lost.
    fn burst(first: u32, count: u32);
    /// Logs a text record, if the level lets it through.
    fn log_text(severity: Severity, message: &'i str);
}

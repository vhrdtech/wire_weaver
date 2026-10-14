//! Test API root for `ww_i2c`: every trait of the crate at one level, used by `tests/io_i2c`.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait IoI2cTest {
    ww_impl!(i2c: ww_i2c::I2c);
    ww_impl!(i2c_watch: ww_i2c::I2cWatch);
    ww_impl!(i2c_monitor: ww_i2c::I2cMonitor);

    /// Test-only: runs one read cycle for every configured `I2cWatch` watch and reports results on `changes`
    /// exactly as a real device's polling loop would, instead of waiting for `period` to pass. Returns how many
    /// watches were read.
    fn tick_watches() -> u8;
}

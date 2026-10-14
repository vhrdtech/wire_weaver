//! Test API root for `ww_i2c`: every trait of the crate at one level, used by `tests/io_i2c`.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait IoI2cTest {
    ww_impl!(i2c: ww_i2c::I2c);
    ww_impl!(i2c_watch: ww_i2c::I2cWatch);
    ww_impl!(i2c_monitor: ww_i2c::I2cMonitor);
}

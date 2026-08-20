#![no_std]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait AllGpioApi {
    ww_impl!(port[]: ww_gpio::Bank);
}

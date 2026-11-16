#![no_std]

use ww_api::prelude::*;

#[ww_api_root]
pub trait AllGpioApi {
    ww_impl!(port[]: ww_gpio::Bank);
}

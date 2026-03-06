#![no_std]

use wire_weaver::prelude::*;

#[ww_trait]
pub trait AllGpioApi {
    ww_impl!(port[]: ww_gpio::Bank);
    fn port_count() -> u32;
}

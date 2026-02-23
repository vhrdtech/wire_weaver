#![no_std]

use wire_weaver::prelude::*;

#[ww_trait]
pub trait AllGpioApi {
    ww_impl!(port[]: "../../ww_stdlib/ww_gpio/src/lib.rs" as ww_gpio::GpioBank);
    fn port_count() -> u32;
}

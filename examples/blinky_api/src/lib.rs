#![no_std]

use wire_weaver::prelude::*;

#[ww_api_root(wire = "w1.0")]
pub trait BlinkyApi {
    fn led_on();
    fn led_off();
}

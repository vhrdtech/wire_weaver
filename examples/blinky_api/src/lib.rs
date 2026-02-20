#![no_std]

use wire_weaver::prelude::*;

#[ww_trait]
pub trait BlinkyApi {
    fn led_on();
    fn led_off();
}

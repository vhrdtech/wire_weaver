#![no_std]

use wire_weaver::prelude::*;

#[ww_api_root(wire = "w1.1")]
pub trait BlinkyApi {
    fn led_on();
    fn led_off();
    #[since = "w1.1"]
    fn led_toggle();
}

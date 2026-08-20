#![no_std]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait BlinkyApi {
    fn led_on();
    fn led_off();
    #[since = "0.1.1"]
    fn led_toggle();
}

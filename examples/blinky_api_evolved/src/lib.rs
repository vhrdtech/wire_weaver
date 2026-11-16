#![no_std]

use ww_api::prelude::*;

#[ww_api_root]
pub trait BlinkyApi {
    fn led_on();
    fn led_off();
    #[since = "w1.1"]
    fn led_toggle();
}

//! The blinky example API in a later version: `blinky_api` with `led_toggle` added `#[since = "w1.1"]`, for the
//! evolution checks (old firmware against a new client, and the other way round).
#![no_std]

use ww_api::prelude::*;

#[ww_api_root]
pub trait BlinkyApi {
    #[effect(actuate)]
    fn led_on();
    #[effect(actuate)]
    fn led_off();
    #[since = "w1.1"]
    #[effect(actuate)]
    fn led_toggle();
}

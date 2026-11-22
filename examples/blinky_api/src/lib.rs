//! API of the blinky example device: an LED turned on and off, the smallest possible API. Start here when new
//! to WireWeaver; `blinky_api_evolved` shows the same API after a change.
#![no_std]

use ww_api::prelude::*;

#[ww_api_root]
pub trait BlinkyApi {
    #[effect(actuate)]
    fn led_on();
    #[effect(actuate)]
    fn led_off();
}

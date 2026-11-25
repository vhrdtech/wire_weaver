//! API of the blinky example device after its first change: `blinky_api` 0.1.1, with `led_toggle` added at wire
//! w1.1. Devices and hosts of 0.1.0 and 0.1.1 still talk to each other (`blinky_evolved` checks it).
#![no_std]

use ww_api::prelude::*;

// --8<-- [start:root]
#[ww_api_root]
pub trait BlinkyApi {
    #[effect(actuate)]
    fn led_on();
    #[effect(actuate)]
    fn led_off();
    /// Turns the LED on when it is off, off when it is on.
    #[since = "w1.1"]
    #[effect(actuate)]
    fn led_toggle();
}
// --8<-- [end:root]

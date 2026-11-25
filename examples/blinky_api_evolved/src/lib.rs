//! The blinky example API in a later version: `blinky_api` with `led_toggle` added `#[since = "w1.1"]`, for the
//! evolution checks (old firmware against a new client, and the other way round).
#![no_std]

use ww_api::prelude::*;
use ww_si::Fixed;
use ww_si::prefix::One;
use ww_si::units::Item;

/// An LED on a board.
#[ww_api_root]
pub trait BlinkyApi {
    /// Turns the LED on.
    #[effect(actuate)]
    fn led_on();
    /// Turns the LED off.
    #[effect(actuate)]
    fn led_off();
    /// Turns the LED on or off, and returns whether it was on before.
    #[effect(actuate)]
    fn set_led(on: bool) -> bool;
    /// How many times the LED went from off to on since the device started.
    #[effect(read)]
    fn blinks() -> Fixed<Item, One, u32>;
    /// Turns the LED on when it is off, and off when it is on.
    #[since = "w1.1"]
    #[effect(actuate)]
    fn led_toggle();
}

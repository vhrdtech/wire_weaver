//! API of the blinky example device after its first change: `blinky_api` 0.1.1, with `led_toggle` added at wire
//! w1.1. Devices and hosts of 0.1.0 and 0.1.1 still talk to each other (`blinky_evolved` checks it).
#![no_std]

use ww_api::prelude::*;
use ww_si::Fixed;
use ww_si::prefix::One;
use ww_si::units::Item;

/// A number of times the LED went on: one step per time, 0 to 4 294 967 295.
pub type Blinks = Fixed<Item, One, u32>;

// --8<-- [start:root]
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
// --8<-- [end:root]

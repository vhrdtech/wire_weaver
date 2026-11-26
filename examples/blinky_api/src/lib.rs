//! API of the blinky example device: an LED that a host turns on and off, the first device of the book. Start here
//! when new to WireWeaver; `blinky_api_evolved` shows the same API after a change.
#![cfg_attr(not(feature = "std"), no_std)]

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
    // --8<-- [start:methods]
    /// Turns the LED on or off, and returns whether it was on before.
    #[effect(actuate)]
    fn set_led(on: bool) -> bool;
    /// How many times the LED went from off to on since the device started.
    #[effect(read)]
    fn blinks() -> Fixed<Item, One, u32>;
    // --8<-- [end:methods]
}
// --8<-- [end:root]

//! An API after a crate breaking bump (0.3.0 -> 0.4.0) that stayed on its wire line: w2.3 -> w2.4 added a method.
//! Used by `tests/wire_line` (EVO-25) against firmware of `wire_line_old_api`.
#![no_std]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait WireLine {
    fn led_on();
    fn led_off();
    /// Added in w2.4: a device on w2.3 doesn't have it.
    #[since = "w2.4"]
    fn led_toggle();
}

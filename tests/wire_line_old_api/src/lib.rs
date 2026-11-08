//! The API of `tests/wire_line` at its old version 0.3.0, wire w2.3, as firmware already in the field runs it.
//!
//! In a real project both are the same crate at two versions; a workspace can't hold two versions of one crate, so
//! the old one is kept here under its own name, and the test device reports it as `wire_line_api` 0.3.0.
#![no_std]

use wire_weaver::prelude::*;

#[ww_api_root(wire = "w2.3")]
pub trait WireLine {
    fn led_on();
    fn led_off();
}

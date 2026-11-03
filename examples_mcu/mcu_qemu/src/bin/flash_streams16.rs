//! Flash budget (TEST-30): a server of `flash_budget_api::Streams16`, see `src/flash_budget.rs`.
#![no_main]
#![no_std]

extern crate panic_semihosting;

#[macro_use]
#[path = "../flash_budget.rs"]
mod flash_budget;

use wire_weaver::prelude::*;

flash_budget_server!(Streams16);

impl ServerState {
    base_handlers!();
    streams!(
        (x0, close_x0, sideband_x0),
        (x1, close_x1, sideband_x1),
        (x2, close_x2, sideband_x2),
        (x3, close_x3, sideband_x3),
        (x4, close_x4, sideband_x4),
        (x5, close_x5, sideband_x5),
        (x6, close_x6, sideband_x6),
        (x7, close_x7, sideband_x7),
        (x8, close_x8, sideband_x8),
        (x9, close_x9, sideband_x9),
        (x10, close_x10, sideband_x10),
        (x11, close_x11, sideband_x11),
        (x12, close_x12, sideband_x12),
        (x13, close_x13, sideband_x13),
        (x14, close_x14, sideband_x14),
        (x15, close_x15, sideband_x15),
    );
}

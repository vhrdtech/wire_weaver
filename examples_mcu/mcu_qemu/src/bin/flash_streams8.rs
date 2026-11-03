//! Flash budget (TEST-30): a server of `flash_budget_api::Streams8`, see `src/flash_budget.rs`.
#![no_main]
#![no_std]

extern crate panic_semihosting;

#[macro_use]
#[path = "../flash_budget.rs"]
mod flash_budget;

use wire_weaver::prelude::*;

flash_budget_server!(Streams8);

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
    );
}

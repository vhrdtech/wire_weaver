//! Flash budget (TEST-30): a server of `flash_budget_api::MaxLenBounds16`, see `src/flash_budget.rs`.
#![no_main]
#![no_std]

extern crate panic_semihosting;

#[macro_use]
#[path = "../flash_budget.rs"]
mod flash_budget;

use wire_weaver::prelude::*;

flash_budget_server!(MaxLenBounds16);

impl ServerState {
    base_handlers!();
    maxlen_props!(
        (get_x0, set_x0, 4),
        (get_x1, set_x1, 8),
        (get_x2, set_x2, 12),
        (get_x3, set_x3, 16),
        (get_x4, set_x4, 20),
        (get_x5, set_x5, 24),
        (get_x6, set_x6, 28),
        (get_x7, set_x7, 32),
        (get_x8, set_x8, 36),
        (get_x9, set_x9, 40),
        (get_x10, set_x10, 44),
        (get_x11, set_x11, 48),
        (get_x12, set_x12, 52),
        (get_x13, set_x13, 56),
        (get_x14, set_x14, 60),
        (get_x15, set_x15, 64),
    );
}

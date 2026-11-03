//! Flash budget (TEST-30): a server of `flash_budget_api::Props16`, see `src/flash_budget.rs`.
#![no_main]
#![no_std]

extern crate panic_semihosting;

#[macro_use]
#[path = "../flash_budget.rs"]
mod flash_budget;

use wire_weaver::prelude::*;

flash_budget_server!(Props16);

impl ServerState {
    base_handlers!();
    props!(
        (get_x0, set_x0),
        (get_x1, set_x1),
        (get_x2, set_x2),
        (get_x3, set_x3),
        (get_x4, set_x4),
        (get_x5, set_x5),
        (get_x6, set_x6),
        (get_x7, set_x7),
        (get_x8, set_x8),
        (get_x9, set_x9),
        (get_x10, set_x10),
        (get_x11, set_x11),
        (get_x12, set_x12),
        (get_x13, set_x13),
        (get_x14, set_x14),
        (get_x15, set_x15),
    );
}

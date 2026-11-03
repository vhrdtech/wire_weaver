//! Flash budget (TEST-30): a server of `flash_budget_api::CallsArgs16`, see `src/flash_budget.rs`.
#![no_main]
#![no_std]

extern crate panic_semihosting;

#[macro_use]
#[path = "../flash_budget.rs"]
mod flash_budget;

use wire_weaver::prelude::*;

flash_budget_server!(CallsArgs16);

impl ServerState {
    base_handlers!();
    calls_args!(
        x0, x1, x2, x3, x4, x5, x6, x7, x8, x9, x10, x11, x12, x13, x14, x15,
    );
}

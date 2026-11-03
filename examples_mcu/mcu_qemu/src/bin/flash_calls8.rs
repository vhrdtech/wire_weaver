//! Flash budget (TEST-30): a server of `flash_budget_api::Calls8`, see `src/flash_budget.rs`.
#![no_main]
#![no_std]

extern crate panic_semihosting;

#[macro_use]
#[path = "../flash_budget.rs"]
mod flash_budget;

use wire_weaver::prelude::*;

flash_budget_server!(Calls8);

impl ServerState {
    base_handlers!();
    calls!(x0, x1, x2, x3, x4, x5, x6, x7,);
}

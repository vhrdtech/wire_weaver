//! Flash budget (TEST-30): a server of `flash_budget_api::Arrayed`, the reference of `qemu-flash-keyed`, see
//! `src/flash_budget.rs`.
#![no_main]
#![no_std]

extern crate panic_semihosting;

#[macro_use]
#[path = "../flash_budget.rs"]
mod flash_budget;

use wire_weaver::prelude::*;

flash_budget_server!(Arrayed);

impl ServerState {
    base_handlers!();
    item_array!();
}

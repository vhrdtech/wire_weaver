//! Flash budget (TEST-30): a server of `flash_budget_api::Globals16` with introspection off, see
//! `src/flash_budget.rs`.
#![no_main]
#![no_std]

extern crate panic_semihosting;

#[macro_use]
#[path = "../flash_budget.rs"]
mod flash_budget;

use wire_weaver::prelude::*;

flash_budget_server!(Globals16, introspect = "disabled");

impl ServerState {
    base_handlers!();
    calls!(
        g0_m, g1_m, g2_m, g3_m, g4_m, g5_m, g6_m, g7_m, g8_m, g9_m, g10_m, g11_m, g12_m, g13_m,
        g14_m, g15_m,
    );
}

//! Flash budget (TEST-30): a server of `flash_budget_api::Typed8`, see `src/flash_budget.rs`.
#![no_main]
#![no_std]

extern crate panic_semihosting;

#[macro_use]
#[path = "../flash_budget.rs"]
mod flash_budget;

use wire_weaver::prelude::*;

flash_budget_server!(Typed8);

impl ServerState {
    base_handlers!();
    calls_typed!(
        (x0, In0, Out0),
        (x1, In1, Out1),
        (x2, In2, Out2),
        (x3, In3, Out3),
        (x4, In4, Out4),
        (x5, In5, Out5),
        (x6, In6, Out6),
        (x7, In7, Out7),
    );
}

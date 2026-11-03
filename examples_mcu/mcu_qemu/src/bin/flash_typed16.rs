//! Flash budget (TEST-30): a server of `flash_budget_api::Typed16`, see `src/flash_budget.rs`.
#![no_main]
#![no_std]

extern crate panic_semihosting;

#[macro_use]
#[path = "../flash_budget.rs"]
mod flash_budget;

use wire_weaver::prelude::*;

flash_budget_server!(Typed16);

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
        (x8, In8, Out8),
        (x9, In9, Out9),
        (x10, In10, Out10),
        (x11, In11, Out11),
        (x12, In12, Out12),
        (x13, In13, Out13),
        (x14, In14, Out14),
        (x15, In15, Out15),
    );
}

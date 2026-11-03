//! Flash budget (TEST-30): a server of `flash_budget_api::Observed4`, plain observed properties, see
//! `src/flash_budget.rs`.
#![no_main]
#![no_std]

extern crate panic_semihosting;

#[macro_use]
#[path = "../flash_budget.rs"]
mod flash_budget;

use static_cell::StaticCell;
use wire_weaver::prelude::*;
use ww_stream::observe::ObserveLimits;

static STATE: StaticCell<api_impl::ObserveState> = StaticCell::new();

flash_budget_observe_server!(
    Observed4,
    [],
    state: api_impl::ObserveState,
    init: api_impl::ObserveState::init(STATE.uninit(), ObserveLimits::DEFAULT)
);

impl ServerState {
    base_handlers!();
    observed!(get_x0, get_x1, get_x2, get_x3);
}

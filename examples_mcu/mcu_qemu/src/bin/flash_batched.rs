//! Flash budget (TEST-30): a server of `flash_budget_api::Batched4` that takes samples of its `+batched` properties:
//! a batcher, an outbox and the `Series` encoder. See `src/flash_budget.rs`.
#![no_main]
#![no_std]

extern crate panic_semihosting;

#[macro_use]
#[path = "../flash_budget.rs"]
mod flash_budget;

use static_cell::StaticCell;
use wire_weaver::prelude::*;
use ww_stream::batch::{DEFAULT_POLICY, Scratch, scratch_bytes};
use ww_stream::observe::{ObserveLimits, Second};

type State = api_impl::ObserveState<&'static mut api_impl::ObserveBatches>;
const SB: usize = scratch_bytes::<Second>(api_impl::OBSERVE_BATCH);

static BATCHES: StaticCell<api_impl::ObserveBatches> = StaticCell::new();
static STATE: StaticCell<State> = StaticCell::new();
static SCRATCH: StaticCell<Scratch<SB>> = StaticCell::new();

flash_budget_observe_server!(
    Batched4,
    [],
    state: State,
    init: State::init(
        STATE.uninit(),
        ObserveLimits::DEFAULT,
        api_impl::ObserveBatches::init(BATCHES.uninit(), DEFAULT_POLICY),
    ),
    scratch: SCRATCH.init(Scratch::new())
);

impl ServerState {
    base_handlers!();
    observed!(get_x0, get_x1, get_x2, get_x3);
}

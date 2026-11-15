//! Flash budget (TEST-30): a server of `flash_budget_api::Keyed`, `qemu-flash-arrayed` with its array keyed
//! (SRV-62), see `src/flash_budget.rs`.
#![no_main]
#![no_std]

extern crate panic_semihosting;

#[macro_use]
#[path = "../flash_budget.rs"]
mod flash_budget;

use wire_weaver::prelude::*;

flash_budget_server!(Keyed);

impl ServerState {
    base_handlers!();
    item_array!();

    #[inline(never)]
    fn item_index_of(
        &mut self,
        _cx: &mut Context<'_, impl Out>,
        key: flash_budget_api::Code,
    ) -> RpcResult<Option<UNib32>> {
        let key = flash_budget::black_box(key);
        flash_budget::black_box(RpcResult::Ready(Some(UNib32(key.0 as u32))))
    }

    #[inline(never)]
    fn get_item_key(
        &mut self,
        _cx: &mut Context<'_, impl Out>,
        index_chain: [ww_client_server::ResourceIndex; 1],
    ) -> GetResult<flash_budget_api::Code, ()> {
        let index = flash_budget::black_box(index_chain[0].get().0);
        flash_budget::black_box(GetResult::Value(flash_budget_api::Code(index as u16)))
    }
}

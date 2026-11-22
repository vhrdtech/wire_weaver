//! Stand-in dependency for `wire_weaver_evolution`'s own `pins::refresh` tests (EVO-70, `tests/check.rs`): a real
//! workspace member, so its definitions resolve the way a real dependency's do, framework types only (`ww_api`).
//! Its `src/lib.rs` is copied and edited in place by those tests to drive a changed dependency; never edited here
//! by hand.
#![no_std]

use ww_api::prelude::*;

#[ww_trait]
#[effect(edit)]
pub trait ValueApi {
    fn value() -> Value<'i>;
}

#[derive_shrink_wrap]
pub struct Value<'i> {
    pub id: MaxLen<RefVec<'i, u8>, 16>,
}

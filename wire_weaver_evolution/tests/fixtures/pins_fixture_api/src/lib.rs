//! Frozen fixture for `wire_weaver_evolution`'s own `pins::refresh` tests (EVO-70, `tests/check.rs`): one embedded
//! trait of a tests/ stand-in dependency (`pins_dep::ValueApi`, framework types only), no `ww_stdlib` crate, so
//! the tests survive the `ww_stdlib` cut (P2401#16nov-0955h).
#![no_std]

use ww_api::prelude::*;

#[ww_api_root]
pub trait PinsFixtureApi {
    ww_impl!(value: pins_dep::ValueApi);
}

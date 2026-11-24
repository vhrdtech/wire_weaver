//! The wasm module example API after a change: `wasm_module_api` with `mul`. A firmware reloads its slot with a
//! module of this API, and a client relearns that slot only.
#![no_std]

use ww_api::prelude::*;

#[ww_api_root]
#[effect(edit)]
pub trait Calc {
    /// `a + b`, wrapping. Plain numbers, no unit.
    fn add(a: u32, b: u32) -> u32;
    /// Traps (`unreachable`): the slot unloads the module and its parent keeps running.
    fn trap();
    /// Never returns: an endless loop, stopped only by an interpreter that meters fuel.
    fn spin();
    /// `a * b`, wrapping: what this version adds.
    #[since = "w1.1"]
    fn mul(a: u32, b: u32) -> u32;
}

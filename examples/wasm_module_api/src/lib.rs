//! API of the wasm module example: a calculator compiled to `wasm32-unknown-unknown` and served from a mount slot of
//! a firmware (`examples_mcu/usb_stm32h725ig`'s `stm32h725ig-wasm-slot`) by a wasm interpreter. `wasm_module_api_evolved`
//! is the module after a change, loaded into the same slot.
#![no_std]

use ww_api::prelude::*;

#[ww_api_root]
#[effect(edit)]
pub trait Calc {
    /// `a + b`, wrapping: the call the interpreters' cycles are measured with. Plain numbers, no unit.
    fn add(a: u32, b: u32) -> u32;
    /// Traps (`unreachable`): the slot unloads the module and its parent keeps running.
    fn trap();
    /// Never returns: an endless loop, stopped only by an interpreter that meters fuel.
    fn spin();
}

//! API of the wasm slot example firmware (`examples_mcu/usb_stm32h725ig`'s `stm32h725ig-wasm-slot`): a mount slot per
//! wasm interpreter, each serving a module image the firmware carries in flash (`wasm_module_api` or
//! `wasm_module_api_evolved`). A firmware built without an interpreter keeps its slot empty.
#![cfg_attr(not(feature = "std"), no_std)]

use ww_api::prelude::*;
use ww_si::Fixed;
use ww_si::prefix::One;
use ww_si::units::{Byte, Cycle, Hertz, Item};

#[ww_api_root]
pub trait WasmSlot {
    /// The module run by wasefire-interpreter.
    mount!(wasefire: wasm_module_api);
    /// The module run by wasmi.
    mount!(wasmi: wasm_module_api);
    /// Loads `image` into the slot of `interpreter`. The module loaded there is unloaded first, so its memory is free
    /// for the new one: a refused load leaves the slot empty.
    #[effect(edit)]
    fn load(interpreter: Interpreter, image: Image) -> Result<(), LoadRefused>;
    /// Empties the slot of `interpreter`.
    #[effect(edit)]
    fn unload(interpreter: Interpreter);
    /// What the slot of `interpreter` dropped, refused or unloaded since boot.
    #[effect(read)]
    fn counters(interpreter: Interpreter) -> SlotCounters;
    /// DWT cycles and stack of the last load and the last request into the slot of `interpreter`.
    #[effect(read)]
    fn timing(interpreter: Interpreter) -> Timing;
    /// The firmware's heap, where the interpreters keep the modules they load.
    #[effect(read)]
    fn heap() -> HeapUse;
    /// Starts the heap's peak over at what is in use now.
    #[effect(edit)]
    fn reset_heap_peak();
}

/// A wasm interpreter the firmware may be built with.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug, PartialEq, Eq), ww_repr = u4)]
pub enum Interpreter {
    Wasefire,
    Wasmi,
}

/// A module image in the firmware's flash.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug, PartialEq, Eq), ww_repr = u4)]
pub enum Image {
    /// `wasm_module_api` 0.1.0.
    Calc,
    /// `wasm_module_api_evolved` 0.1.1: `Calc` with `mul`.
    CalcEvolved,
}

/// Why a module wasn't loaded; the slot is left empty.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug, PartialEq, Eq), ww_repr = u4)]
pub enum LoadRefused {
    /// The firmware was built without this interpreter.
    NotBuilt,
    /// The interpreter refused the wasm binary (invalid, an import or export missing, out of memory).
    Instantiate,
    /// The slot refused the module's model.
    Model,
    /// The firmware carries no such image.
    NoImage,
}

/// What a slot dropped, refused or unloaded since boot (`ww_device::mount::ModuleCounters`).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct SlotCounters {
    /// Modules unloaded for a fault: a trap, out of fuel, a reply written past its buffer.
    pub faults: Fixed<Item, One, u32>,
    /// Requests refused (`Overloaded`, too long) or answered `Unavailable`.
    pub refused: Fixed<Item, One, u32>,
    /// Module events dropped: malformed, stray, no room.
    pub dropped: Fixed<Item, One, u32>,
}

/// DWT cycles and stack of the last load and the last request into a slot, at `cpu_clock`. Stack is what the call
/// used below the handler's own frame, interrupts that came in meanwhile included.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct Timing {
    /// The interpreter validating and instantiating the image, and reading the module's buffers.
    pub instantiate: Fixed<Cycle, One, u32>,
    pub instantiate_stack: Fixed<Byte, One, u32>,
    /// The slot taking the module: its model read, its trait table built, the module loaded before dropped.
    pub load: Fixed<Cycle, One, u32>,
    pub load_stack: Fixed<Byte, One, u32>,
    /// The slot's whole `forward`: the path taken off, the module called, its reply checked and sent.
    pub forward: Fixed<Cycle, One, u32>,
    /// The module's `process` alone: the request copied in, the interpreter run, the reply copied out.
    pub process: Fixed<Cycle, One, u32>,
    pub forward_stack: Fixed<Byte, One, u32>,
    /// The CPU clock the cycles count.
    pub cpu_clock: Fixed<Hertz, One, u32>,
}

/// The firmware's heap.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct HeapUse {
    pub used: Fixed<Byte, One, u32>,
    /// The most in use at once since boot or the last `reset_heap_peak`.
    pub peak: Fixed<Byte, One, u32>,
    pub size: Fixed<Byte, One, u32>,
}

//! A mount slot per interpreter, the module images in flash, and what is measured around them: DWT cycles, the stack
//! a call uses below the handler, and the heap the interpreters allocate from.

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};

use embedded_alloc::LlffHeap;
use ww_device::mount::{Module, ModuleError, ModuleSlot, Outbox};

/// The longest request and reply through a slot: the module's buffers (`ww_wasm::guest_module!`'s `buf_len`).
pub const MSG: usize = 512;
/// Room for a module's events in one turn.
pub const OUT: usize = 1024;

/// `wasm_module_api` 0.1.0 and `wasm_module_api_evolved` 0.1.1, built by `just wasm-modules`.
pub static CALC: &[u8] = include_bytes!("../../../modules/calc.wasm");
pub static CALC_EVOLVED: &[u8] = include_bytes!("../../../modules/calc_evolved.wasm");

/// The memory a wasefire-interpreter module gets: its 4 KiB stack and its data (`examples_mcu/wasm_module`).
#[cfg(feature = "wasefire")]
pub const WASEFIRE_MEMORY: usize = 8 * 1024;

#[cfg(feature = "wasefire")]
pub type WasefireModule = Timed<ww_wasm::wasefire::WasefireModule>;
#[cfg(not(feature = "wasefire"))]
pub type WasefireModule = NotBuilt;
#[cfg(feature = "wasmi")]
pub type WasmiModule = Timed<ww_wasm::wasmi::WasmiModule>;
#[cfg(not(feature = "wasmi"))]
pub type WasmiModule = NotBuilt;

/// The module of an interpreter the firmware was built without: never loaded.
#[cfg(not(all(feature = "wasefire", feature = "wasmi")))]
pub enum NotBuilt {}

#[cfg(not(all(feature = "wasefire", feature = "wasmi")))]
impl Module for NotBuilt {
    fn model(&self) -> &[u8] {
        match *self {}
    }

    fn process<'r>(&mut self, _: &[u8], _: &mut Outbox<'_>, _: &'r mut [u8]) -> Result<&'r [u8], ModuleError> {
        match *self {}
    }
}

/// A module whose `process` is timed with the DWT cycle counter.
#[cfg(any(feature = "wasefire", feature = "wasmi"))]
pub struct Timed<M> {
    pub module: M,
    /// Cycles of the last `process`.
    pub process: u32,
}

#[cfg(any(feature = "wasefire", feature = "wasmi"))]
impl<M: Module> Module for Timed<M> {
    fn model(&self) -> &[u8] {
        self.module.model()
    }

    fn process<'r>(
        &mut self,
        request: &[u8],
        events: &mut Outbox<'_>,
        reply: &'r mut [u8],
    ) -> Result<&'r [u8], ModuleError> {
        let start = cortex_m::peripheral::DWT::cycle_count();
        let result = self.module.process(request, events, reply);
        self.process = cortex_m::peripheral::DWT::cycle_count().wrapping_sub(start);
        result
    }

    fn poll(&mut self, events: &mut Outbox<'_>) -> Result<(), ModuleError> {
        self.module.poll(events)
    }
}

/// What was measured last on a slot, in cycles and bytes.
#[derive(Default, Clone, Copy)]
pub struct Measured {
    pub instantiate: u32,
    pub instantiate_stack: u32,
    pub load: u32,
    pub load_stack: u32,
    pub forward: u32,
    pub process: u32,
    pub forward_stack: u32,
}

/// One slot and what was measured on it.
pub struct Slot<M: 'static> {
    pub slot: &'static mut ModuleSlot<M, MSG, OUT>,
    pub measured: Measured,
}

/// The heap the interpreters allocate from, with the most it held at once.
pub struct Heap {
    heap: LlffHeap,
    peak: AtomicUsize,
}

pub const HEAP_SIZE: usize = 192 * 1024;

#[global_allocator]
pub static HEAP: Heap = Heap {
    heap: LlffHeap::empty(),
    peak: AtomicUsize::new(0),
};

impl Heap {
    /// Gives the heap its memory, once, before the first allocation.
    pub fn init(&self) {
        static mut MEMORY: [u8; HEAP_SIZE] = [0; HEAP_SIZE];
        // SAFETY: called once at boot, before any allocation; MEMORY is used by the heap only
        unsafe { self.heap.init(core::ptr::addr_of_mut!(MEMORY) as usize, HEAP_SIZE) }
    }

    pub fn used(&self) -> usize {
        self.heap.used()
    }

    pub fn peak(&self) -> usize {
        self.peak.load(Ordering::Relaxed)
    }

    pub fn reset_peak(&self) {
        self.peak.store(self.used(), Ordering::Relaxed);
    }
}

// SAFETY: every call goes to LlffHeap as it came; the peak is bookkeeping only
unsafe impl GlobalAlloc for Heap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's contract, passed on
        let ptr = unsafe { self.heap.alloc(layout) };
        self.peak.fetch_max(self.heap.used(), Ordering::Relaxed);
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller's contract, passed on
        unsafe { self.heap.dealloc(ptr, layout) }
    }
}

/// Bytes of stack painted below the caller's frame before a measured call.
const PAINT: usize = 48 * 1024;
const PAINT_WORD: u32 = 0xA5A5_5A5A;
/// The bottom of the stack: flip-link puts it at the start of RAM, below the statics.
const STACK_BOTTOM: usize = 0x2400_0000;
/// Left alone just below the stack pointer: the frame of the painting itself.
const MARGIN: usize = 256;

/// Paints the stack below the caller, for [stack_used]. Returns where the paint ends.
#[inline(never)]
pub fn paint_stack() -> usize {
    let sp = cortex_m::register::msp::read() as usize;
    let top = (sp - MARGIN) & !3;
    let bottom = top.saturating_sub(PAINT).max(STACK_BOTTOM + MARGIN);
    let mut addr = bottom;
    while addr < top {
        // SAFETY: below the stack pointer and above the stack's bottom: memory no live frame uses
        unsafe { core::ptr::write_volatile(addr as *mut u32, PAINT_WORD) };
        addr += 4;
    }
    top
}

/// The stack used below the caller of [paint_stack] since it painted, `top` being what it returned: the margin left
/// alone counts as used once anything below it is.
#[inline(never)]
pub fn stack_used(top: usize) -> u32 {
    let bottom = top.saturating_sub(PAINT).max(STACK_BOTTOM + MARGIN);
    let mut addr = bottom;
    // SAFETY: the painted range, read only
    while addr < top && unsafe { core::ptr::read_volatile(addr as *const u32) } == PAINT_WORD {
        addr += 4;
    }
    if addr == top { 0 } else { (top + MARGIN - addr) as u32 }
}

//! The fixed way of the dynamic-prefix bench ([dyn_prefix](../dyn_prefix.rs)), `DeltaPacked`, Cortex-M3. Run under
//! `qemu_icount`: the first `mark_start`/`mark_end` pair is calibration, then one pair per `Series`; the bin
//! prints the bytes per batch and group. A fault exits QEMU with an error instead of looping.
#![no_main]
#![no_std]

extern crate panic_semihosting;

#[path = "../dyn_prefix.rs"]
mod dyn_prefix;
#[path = "../dyn_prefix_qemu.rs"]
mod dyn_prefix_qemu;
#[path = "../foc_signals.rs"]
mod foc_signals;

use cortex_m_rt::{ExceptionFrame, entry, exception};
use cortex_m_semihosting::debug;
use dyn_prefix::Group;
use wire_weaver::shrink_wrap::series::DeltaPacked;

#[exception]
unsafe fn HardFault(_frame: &ExceptionFrame) -> ! {
    debug::exit(debug::EXIT_FAILURE);
    loop {}
}

#[entry]
fn main() -> ! {
    dyn_prefix_qemu::Marks::main(&Group::FIXED, |marks| {
        dyn_prefix::run_fixed(DeltaPacked, marks)
    })
}

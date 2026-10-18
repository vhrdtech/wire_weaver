//! The shared clock's columns of one FOC level batch (21 x `Series<T>` + one `Series<Second>`, n = 32, two
//! batches) by the item walk and written straight from a `SeriesStore` (experiment series-memcpy-fast),
//! Cortex-M3, under `DeltaPacked` and `RawPlain` in that order. Run under `qemu_icount`: the first
//! `mark_start`/`mark_end` pair is calibration, then per policy and batch, per property and then the clock, one
//! pair for the walk and one for the store; the bin prints the bytes per policy, batch and group. A fault exits
//! QEMU with an error instead of looping.
#![no_main]
#![no_std]

extern crate panic_semihosting;

#[path = "../foc_signals.rs"]
mod foc_signals;
#[path = "../shared_clock.rs"]
mod shared_clock;

use core::fmt::Write;
use cortex_m_rt::{ExceptionFrame, entry, exception};
use cortex_m_semihosting::{debug, hio};
use shared_clock::{BATCHES, Group};
use wire_weaver::shrink_wrap::series::{DeltaPacked, RawPlain, SeriesPolicy};

#[inline(never)]
fn mark_start() {
    unsafe { core::arch::asm!("nop", options(nostack, preserves_flags)) }
}

#[inline(never)]
fn mark_end() {
    unsafe { core::arch::asm!("nop", "nop", options(nostack, preserves_flags)) }
}

/// Bytes per batch and group.
struct Marks([[usize; 6]; BATCHES]);

impl shared_clock::Meter for Marks {
    fn measure(&mut self, group: Group, batch: usize, work: &mut dyn FnMut() -> usize) {
        mark_start();
        let bytes = work();
        mark_end();
        self.0[batch][group as usize] += bytes;
    }
}

fn policy<P: SeriesPolicy + Copy>(out: &mut hio::HostStream, name: &str, policy: P) {
    let mut marks = Marks([[0; 6]; BATCHES]);
    shared_clock::run_store(policy, &mut marks);
    for (batch, bytes) in marks.0.iter().enumerate() {
        for group in Group::STORE {
            writeln!(
                out,
                "policy={name} batch={batch} {} bytes={}",
                group.label(),
                bytes[group as usize]
            )
            .unwrap();
        }
    }
}

#[exception]
unsafe fn HardFault(_frame: &ExceptionFrame) -> ! {
    debug::exit(debug::EXIT_FAILURE);
    loop {}
}

#[entry]
fn main() -> ! {
    let mut stdout = hio::hstdout().unwrap();
    mark_start();
    mark_end();
    policy(&mut stdout, "DeltaPacked", DeltaPacked);
    policy(&mut stdout, "RawPlain", RawPlain);
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}

//! Shared clock measurement (idea I-2) with `RawPlain`, Cortex-M3: one FOC level batch of 32 samples of 21 properties
//! as 21 x `Series<Timed<T>>` against 21 x `Series<T>` + one `Series<Second>`, two batches. Run under
//! `qemu_icount`: the first `mark_start`/`mark_end` pair is calibration, then one pair per `Series`, per batch 21
//! timed, 21 untimed and the clock; the bin prints the bytes per batch and group. A fault exits QEMU with an error
//! instead of looping.
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
use wire_weaver::shrink_wrap::series::RawPlain;

#[inline(never)]
fn mark_start() {
    unsafe { core::arch::asm!("nop", options(nostack, preserves_flags)) }
}

#[inline(never)]
fn mark_end() {
    unsafe { core::arch::asm!("nop", "nop", options(nostack, preserves_flags)) }
}

/// Bytes per batch and group.
struct Marks([[usize; 3]; BATCHES]);

impl shared_clock::Meter for Marks {
    fn measure(&mut self, group: Group, batch: usize, work: &mut dyn FnMut() -> usize) {
        mark_start();
        let bytes = work();
        mark_end();
        self.0[batch][group as usize] += bytes;
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
    writeln!(stdout, "policy=RawPlain").unwrap();
    mark_start();
    mark_end();
    let mut marks = Marks([[0; 3]; BATCHES]);
    shared_clock::run(RawPlain, &mut marks);
    for (batch, bytes) in marks.0.iter().enumerate() {
        for group in Group::ALL {
            writeln!(
                stdout,
                "batch={batch} {} bytes={}",
                group.label(),
                bytes[group as usize]
            )
            .unwrap();
        }
    }
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}

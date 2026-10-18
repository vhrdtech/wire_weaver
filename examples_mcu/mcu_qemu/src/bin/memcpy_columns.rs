//! Upper bound of an aligned-plain `Series` column (experiment series-memcpy-fast), Cortex-M3: one FOC level batch
//! (21 properties + the clock, n = 32) copied from struct-of-arrays storage as little-endian payloads, bare and
//! with a real column's framing, two batches. Run under `qemu_icount`: the first `mark_start`/`mark_end` pair is
//! calibration, then per batch one pair for the bare copy and one for the framed columns; the bin prints the bytes.
//! A fault exits QEMU with an error instead of looping.
#![no_main]
#![no_std]

extern crate panic_semihosting;

#[path = "../foc_signals.rs"]
mod foc_signals;
#[path = "../memcpy_columns.rs"]
mod memcpy_columns;

use core::fmt::Write;
use cortex_m_rt::{ExceptionFrame, entry, exception};
use cortex_m_semihosting::{debug, hio};
use memcpy_columns::{BATCHES, Shape};

#[inline(never)]
fn mark_start() {
    unsafe { core::arch::asm!("nop", options(nostack, preserves_flags)) }
}

#[inline(never)]
fn mark_end() {
    unsafe { core::arch::asm!("nop", "nop", options(nostack, preserves_flags)) }
}

/// Bytes per batch and shape.
struct Marks([[usize; 2]; BATCHES]);

impl memcpy_columns::Meter for Marks {
    fn measure(&mut self, shape: Shape, batch: usize, work: &mut dyn FnMut() -> usize) {
        mark_start();
        let bytes = work();
        mark_end();
        self.0[batch][shape as usize] = bytes;
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
    let mut marks = Marks([[0; 2]; BATCHES]);
    memcpy_columns::run(&mut marks);
    for (batch, bytes) in marks.0.iter().enumerate() {
        for shape in Shape::ALL {
            writeln!(stdout, "batch={batch} {} bytes={}", shape.label(), bytes[shape as usize]).unwrap();
        }
    }
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}

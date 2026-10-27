//! What `ww_si::Fixed` costs against the plain fields it replaces ([unit_budget], TEST-27), Cortex-M3
//! (`lm3s6965evb`). Run under `qemu_icount`: the first `mark_start`/`mark_end` pair is calibration, then one pair
//! per measured run, in the order the bin prints them (`bracket=..` lines); then the bytes and the FNV hash of each
//! case (a plain/Fixed pair has the same hash). A fault exits QEMU with an error instead of looping. The H725 twin
//! (DWT cycles) is `stm32h725ig-unit-budget-cycles`.
#![no_main]
#![no_std]

extern crate panic_semihosting;

#[path = "../unit_budget.rs"]
mod unit_budget;

use core::fmt::Write;
use cortex_m_rt::{ExceptionFrame, entry, exception};
use cortex_m_semihosting::{debug, hio};
use unit_budget::Case;

#[inline(never)]
fn mark_start() {
    unsafe { core::arch::asm!("nop", options(nostack, preserves_flags)) }
}

#[inline(never)]
fn mark_end() {
    unsafe { core::arch::asm!("nop", "nop", options(nostack, preserves_flags)) }
}

/// Brackets each run and says which one it is.
struct Marks {
    stdout: hio::HostStream,
    n: usize,
}

impl unit_budget::Meter for Marks {
    fn measure(&mut self, case: Case, run: usize, work: &mut dyn FnMut()) {
        writeln!(
            self.stdout,
            "bracket={} {} run={run} ops={}",
            self.n,
            case.label(),
            case.ops()
        )
        .unwrap();
        self.n += 1;
        mark_start();
        work();
        mark_end();
    }
}

#[exception]
unsafe fn HardFault(_frame: &ExceptionFrame) -> ! {
    debug::exit(debug::EXIT_FAILURE);
    loop {}
}

#[entry]
fn main() -> ! {
    let stdout = hio::hstdout().unwrap();
    mark_start();
    mark_end();
    let mut marks = Marks { stdout, n: 0 };
    let reports = unit_budget::run(&mut marks);
    for case in Case::ALL {
        let r = &reports[case as usize];
        writeln!(
            marks.stdout,
            "{} | bytes={} hash={:08x}",
            case.label(),
            r.bytes,
            r.hash
        )
        .unwrap();
    }
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}

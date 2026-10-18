//! High-rate observe (API-50 cells, SW-64 column-wise `DeltaPacked`), Cortex-M4F (`mps2-an386`): a few values of a
//! power converter observed every switching period (3 at 400 kHz and 1 MHz, 2 at 2 MHz), each cells row, each
//! `poll_properties`, and the raw-copy reference ([high_rate]). Run under `qemu_icount`: the first
//! `mark_start`/`mark_end` pair is calibration, then one pair per measured call, in the order the bin prints them
//! (`bracket=..` lines); then the bytes sent and the RAM of the level's samples. A fault exits QEMU with an error
//! instead of looping. The H725 twin (DWT cycles) is `stm32h725ig-high-rate-cycles`.
#![no_main]
#![no_std]

extern crate panic_semihosting;

#[path = "../../../mcu_qemu/src/high_rate.rs"]
mod high_rate;
#[path = "../../../mcu_qemu/src/power_signals.rs"]
mod power_signals;

use core::fmt::Write;
use cortex_m_rt::{ExceptionFrame, entry, exception};
use cortex_m_semihosting::{debug, hio};
use high_rate::{Case, Run};

#[inline(never)]
fn mark_start() {
    unsafe { core::arch::asm!("nop", options(nostack, preserves_flags)) }
}

#[inline(never)]
fn mark_end() {
    unsafe { core::arch::asm!("nop", "nop", options(nostack, preserves_flags)) }
}

/// Brackets each call and says which one it is.
struct Marks {
    stdout: hio::HostStream,
    n: usize,
}

impl high_rate::Meter for Marks {
    fn measure(&mut self, run: Run, case: Case, k: usize, work: &mut dyn FnMut()) {
        writeln!(
            self.stdout,
            "bracket={} {} | {} k={k}",
            self.n,
            run.label(),
            case.label()
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
    let reports = high_rate::run(&mut marks);
    for run in Run::ALL {
        let r = &reports[run as usize];
        writeln!(
            marks.stdout,
            "{} | updates={} bytes={} hash={:08x} raw bytes per row={}",
            run.label(),
            r.updates,
            r.bytes,
            r.hash,
            run.raw_row_bytes()
        )
        .unwrap();
    }
    let ram = high_rate::ram();
    writeln!(marks.stdout, "ram three={} two={}", ram[0], ram[1]).unwrap();
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}

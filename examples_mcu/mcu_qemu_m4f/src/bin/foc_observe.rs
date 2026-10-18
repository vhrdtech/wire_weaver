//! Observe cells against `<Level>Sampler::sample` (API-50), Cortex-M4F (`mps2-an386`; the Cortex-M3 twin is `qemu-foc-observe`): one row of the motor demo's 21 FOC
//! properties per call, the gate closed and open, and every `poll_properties` ([foc_observe]). Run under
//! `qemu_icount`: the first `mark_start`/`mark_end` pair is calibration, then one pair per measured call, in the
//! order the bin prints them (`bracket=..` lines); then the bytes sent, the stack per call and the RAM of the
//! level's samples. A fault exits QEMU with an error instead of looping.
#![no_main]
#![no_std]

extern crate panic_semihosting;

#[path = "../../../mcu_qemu/src/foc_observe.rs"]
mod foc_observe;
#[path = "../../../mcu_qemu/src/foc_signals.rs"]
mod foc_signals;

use core::fmt::Write;
use cortex_m_rt::{ExceptionFrame, entry, exception};
use cortex_m_semihosting::{debug, hio};
use foc_observe::{Case, Variant};

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

impl foc_observe::Meter for Marks {
    fn measure(&mut self, variant: Variant, case: Case, k: usize, work: &mut dyn FnMut()) {
        writeln!(
            self.stdout,
            "bracket={} {} {} k={k}",
            self.n,
            variant.label(),
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
    let reports = foc_observe::run(&mut marks);
    for (v, r) in Variant::ALL.iter().zip(&reports) {
        writeln!(
            marks.stdout,
            "{} updates={} bytes={} few updates={} bytes={} hash={:08x} stack row={} poll={} setup={} ram={}",
            v.label(),
            r.updates,
            r.bytes,
            r.few_updates,
            r.few_bytes,
            r.hash,
            r.stack[Case::Row as usize],
            r.stack[Case::Poll as usize],
            r.setup,
            r.ram
        )
        .unwrap();
    }
    debug::exit(if reports[0].hash == reports[1].hash {
        debug::EXIT_SUCCESS
    } else {
        debug::EXIT_FAILURE
    });
    loop {}
}

//! Shared body of the `qemu-series-encode-cpu-*` bins (TEST-23): encodes the recorded `i_a` signal
//! ([super::motor_signal]) as `Series<Timed<Ampere>>` at n = 8/32/64 under one fixed [CodecPolicy], prints the
//! encoded length, then exits QEMU. One bin per codec set, so `cargo size` / `llvm-size` of each ELF is that
//! codec set's own flash, not every codec linked together.
//!
//! Semihosting `SYS_ELAPSED` is host wall-clock time (`get_clock()` in QEMU's `arm-compat-semi.c`), not tied to
//! `-icount` at all, so it cannot time an encode in instructions (an earlier version of this file assumed it
//! could; it measured 0 every time). Instructions are counted from outside instead: [mark_start] and [mark_end]
//! bracket the measured call with two distinctly-sized, `#[inline(never)]` marker functions at fixed addresses;
//! running under `qemu-system-arm -d exec,nochain -singlestep -D <tracefile>` logs one trace line per retired
//! instruction, so the instruction count is the line distance between one `mark_start` call and the matching
//! `mark_end` call (both found by address, via `nm --demangle` on the release ELF). The first of the four
//! `mark_start`/`mark_end` occurrences brackets nothing (calibration: the fixed cost of the two calls
//! themselves, to subtract from the other three); the next three bracket n = 8, 32, 64 in that order.

use core::fmt::Write;
use cortex_m_semihosting::{debug, hio};
use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::Series;
use wire_weaver::shrink_wrap::series::SeriesPolicy;
use ww_si::{Ampere, Second, Timed};

const BATCH_SIZES: [usize; 3] = [8, 32, 64];

#[inline(never)]
fn mark_start() {
    unsafe { core::arch::asm!("nop", options(nostack, preserves_flags)) }
}

#[inline(never)]
fn mark_end() {
    unsafe { core::arch::asm!("nop", "nop", options(nostack, preserves_flags)) }
}

fn items() -> [Timed<Ampere>; 64] {
    core::array::from_fn(|i| {
        Timed::new(
            Second::micro(super::motor_signal::T0_US + i as u32 * super::motor_signal::DT_US),
            Ampere::milli(super::motor_signal::I_A_MILLIAMPS[i]),
        )
    })
}

/// The same bin without a `Series`: the items and the printing, what the flash of a codec set is measured against.
#[allow(dead_code)]
pub fn baseline() -> ! {
    let mut stdout = hio::hstdout().unwrap();
    let items = core::hint::black_box(items());
    for &n in &BATCH_SIZES {
        let bytes = core::hint::black_box(&items[..n]).len();
        writeln!(stdout, "n={n} bytes={bytes}").unwrap();
    }
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}

/// `policy` a run-time [CodecPolicy](wire_weaver::shrink_wrap::series::CodecPolicy) or a compile-time policy type.
#[allow(dead_code)]
pub fn run<P: SeriesPolicy + Copy>(policy_name: &str, policy: P) -> ! {
    let mut stdout = hio::hstdout().unwrap();
    let items = items();

    writeln!(stdout, "policy={policy_name}").unwrap();
    mark_start();
    mark_end();
    for &n in &BATCH_SIZES {
        let series = Series::with_policy(&items[..n], policy);
        let mut buf = [0u8; 512];
        mark_start();
        let encoded = series.to_ww_bytes(&mut buf).unwrap();
        mark_end();
        writeln!(stdout, "n={n} bytes={}", encoded.len()).unwrap();
    }

    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}

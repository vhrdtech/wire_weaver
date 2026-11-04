//! Instructions of `ww_si::Quantity` conversions and comparisons at a run-time prefix (STD-102), Cortex-M3
//! (`lm3s6965evb`). Run under `qemu_icount`: the first `mark_start`/`mark_end` pair is calibration, then one pair per
//! case, in the order the bin prints them (`bracket=..` lines), each case called once with values the compiler
//! can't see. A fault exits QEMU with an error instead of looping.
#![no_main]
#![no_std]

extern crate panic_semihosting;

use core::fmt::Write;
use core::hint::black_box;
use cortex_m_rt::{ExceptionFrame, entry, exception};
use cortex_m_semihosting::{debug, hio};
use wire_weaver::prelude::*;
use ww_si::exact_units::FullScaleBits;
use ww_si::{Overflow, Prefix, Quantity, RangeDec, Rounding, Second, Volt, prefix, units};

type Byte = Quantity<units::Byte>;

/// `0 V ..= 3.3 V`: the bound a decode checks.
type Rail = Quantity<units::Volt, RangeDec<0, 33, -1>>;

#[inline(never)]
fn mark_start() {
    unsafe { core::arch::asm!("nop", options(nostack, preserves_flags)) }
}

#[inline(never)]
fn mark_end() {
    unsafe { core::arch::asm!("nop", "nop", options(nostack, preserves_flags)) }
}

#[exception]
unsafe fn HardFault(_frame: &ExceptionFrame) -> ! {
    debug::exit(debug::EXIT_FAILURE);
    loop {}
}

#[inline(never)]
fn mv_to_v(v: &Volt) -> Option<u32> {
    v.to_in::<u32>(Prefix::Unit, Rounding::Truncate, Overflow::Checked)
}

#[inline(never)]
fn ms_to_us(t: &Second) -> Option<u32> {
    t.to_in::<u32>(Prefix::Micro, Rounding::Truncate, Overflow::Checked)
}

#[inline(never)]
fn us_wrapping(t: &Second) -> Option<u32> {
    t.to_in::<u32>(Prefix::Micro, Rounding::Truncate, Overflow::Wrap)
}

#[inline(never)]
fn kib_to_bytes(b: &Byte) -> Option<u32> {
    b.to_in::<u32>(Prefix::Unit, Rounding::Truncate, Overflow::Checked)
}

#[inline(never)]
fn full_scale_to_mv(v: &Volt) -> Option<u32> {
    v.to_in::<u32>(Prefix::Milli, Rounding::Truncate, Overflow::Checked)
}

#[inline(never)]
fn less(a: &Second, b: &Second) -> bool {
    a < b
}

#[inline(never)]
fn bounded_decode(bytes: &[u8]) -> bool {
    Rail::from_ww_bytes(bytes).is_ok()
}

#[inline(never)]
fn to_fixed(t: &Second) -> Option<u32> {
    t.to_fixed::<prefix::Micro, u32, ww_si::Unbounded>()
        .map(|f| f.get())
}

#[inline(never)]
fn step(t: &Second) -> Option<u32> {
    let next = t.steps(&Second::micro(40u32))?.nth(1)?;
    next.to_in::<u32>(Prefix::Micro, Rounding::Truncate, Overflow::Wrap)
}

#[entry]
fn main() -> ! {
    let mut stdout = hio::hstdout().unwrap();
    let mv = black_box(Volt::milli(3300u32));
    let ms = black_box(Second::milli(5u32));
    let us = black_box(Second::micro(1_234_567u32));
    let kib = black_box(Byte::new(Prefix::Kibi, 3u32));
    let fs = black_box(Volt::new(
        Prefix::FullScale(FullScaleBits::new(12).unwrap()),
        2048u32,
    ));
    let mut buf = [0u8; 16];
    let rail = Volt::milli(3300u32).to_ww_bytes(&mut buf).unwrap();

    let mut out = [0u32; 9];
    mark_start();
    mark_end();
    let mut n = 1;
    let mut case = |name: &str, f: &mut dyn FnMut() -> u32, out: &mut u32| {
        writeln!(stdout, "bracket={n} {name}").unwrap();
        n += 1;
        mark_start();
        *out = f();
        mark_end();
    };
    case("mV->V u32", &mut || mv_to_v(&mv).unwrap_or(0), &mut out[0]);
    case(
        "ms->us u32",
        &mut || ms_to_us(&ms).unwrap_or(0),
        &mut out[1],
    );
    case(
        "us wrapping u32",
        &mut || us_wrapping(&us).unwrap_or(0),
        &mut out[2],
    );
    case(
        "KiB->B u32",
        &mut || kib_to_bytes(&kib).unwrap_or(0),
        &mut out[3],
    );
    case(
        "full scale->mV u32",
        &mut || full_scale_to_mv(&fs).unwrap_or(0),
        &mut out[4],
    );
    case("us < ms", &mut || less(&us, &ms) as u32, &mut out[5]);
    case(
        "bounded decode",
        &mut || bounded_decode(black_box(rail)) as u32,
        &mut out[6],
    );
    case(
        "to_fixed us u32",
        &mut || to_fixed(&us).unwrap_or(0),
        &mut out[7],
    );
    case("steps nth(1)", &mut || step(&us).unwrap_or(0), &mut out[8]);
    writeln!(stdout, "results={out:?}").unwrap();

    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}

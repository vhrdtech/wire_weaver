//! Fixture firmware for the no-float guard (TEST-33): every device-relevant `ww_si::Quantity` operation (`to_in` with
//! `Truncate` / `NearestAway` and `Checked` / `Wrap`, `rescaled`, `PartialOrd`, a bounded decode, `to_fixed`, `Timed`,
//! `steps`) at a run-time prefix, binary and full-scale prefixes too, so the host tool (`examples_mcu/fw_budget --flash`, `src/no_f64_guard.rs`) can list
//! what f64 soft-float and 128-bit helper symbols these ops link into the ELF, at a run-time prefix and with the
//! number features firmware actually enables (`guard-u64` adds `ww_si/u64`, as usb_io_fw's 64-bit `Integer` does; `--no-default-features` drops `f32`).
//! Never run under QEMU: only its ELF is read.
#![no_main]
#![no_std]

extern crate panic_semihosting;

use core::hint::black_box;
use cortex_m_rt::entry;
use cortex_m_semihosting::debug;
use wire_weaver::prelude::*;
use ww_si::exact_units::FullScaleBits;
use ww_si::{
    Overflow, Prefix, Quantity, RangeDec, Rounding, Second, Timed, Unbounded, Volt, prefix, units,
};

/// `0 V ..= 3.3 V`, as `tests/bounds.rs`' `Rail` (STD-59): the bound a decode must check.
type Rail = Quantity<units::Volt, RangeDec<0, 33, -1>>;

#[inline(never)]
fn to_in(v: &Volt) -> Option<u32> {
    v.to_in::<u32>(Prefix::Milli, Rounding::Truncate, Overflow::Checked)
}

#[inline(never)]
fn to_in_nearest_i32(v: &Volt) -> Option<i32> {
    v.to_in::<i32>(Prefix::Unit, Rounding::NearestAway, Overflow::Saturate)
}

#[inline(never)]
fn to_in_wrapping(v: &Second) -> Option<u32> {
    v.to_in::<u32>(Prefix::Micro, Rounding::Truncate, Overflow::Wrap)
}

#[inline(never)]
fn rescaled(v: &Volt) -> Option<Volt> {
    v.rescaled(Prefix::Micro, Rounding::NearestEven, Overflow::Checked)
}

#[inline(never)]
fn ordered(a: &Second, b: &Second) -> bool {
    a < b
}

#[inline(never)]
fn bounded_decode(bytes: &[u8]) -> bool {
    Rail::from_ww_bytes(bytes).is_ok()
}

#[inline(never)]
fn to_fixed(v: &Second) -> Option<u32> {
    v.to_fixed::<prefix::Micro, u32, Unbounded>()
        .map(|f| f.get())
}

#[inline(never)]
fn timed_roundtrip(buf: &mut [u8], t: &Timed<Rail>) -> bool {
    let Ok(bytes) = t.to_ww_bytes(buf) else {
        return false;
    };
    Timed::<Rail>::from_ww_bytes(bytes).is_ok()
}

#[inline(never)]
fn steps(v: &Second) -> Option<u32> {
    v.steps(&Second::micro(40u32))?.next()?.to_in::<u32>(
        Prefix::Micro,
        Rounding::Truncate,
        Overflow::Wrap,
    )
}

#[entry]
fn main() -> ! {
    let volt = black_box(Volt::milli(3300u32));
    // a 12-bit full scale and a binary prefix: the kernel's other ratios
    let full_scale = black_box(Volt::new(
        Prefix::FullScale(FullScaleBits::new(12).unwrap()),
        2048u32,
    ));
    let kibi = black_box(Volt::new(Prefix::Kibi, 3u32));
    let t0 = black_box(Second::micro(1_234_567u32));
    let t1 = black_box(Second::milli(5u32));
    let rail = Rail::try_new(Prefix::Milli, 3300u32).unwrap();

    let mut in_range = [0u8; 16];
    let in_range = Volt::milli(3300u32).to_ww_bytes(&mut in_range).unwrap();
    let mut out_of_range = [0u8; 16];
    let out_of_range = Volt::milli(3301u32).to_ww_bytes(&mut out_of_range).unwrap();

    let timed = Timed::new(black_box(ww_si::origins::DeviceMicros::from_raw(1_234_567u32)), rail);
    let mut tbuf = [0u8; 32];

    let mut sum: u32 = 0;
    sum ^= black_box(to_in(&volt)).unwrap_or(0);
    sum ^= black_box(to_in(&full_scale)).unwrap_or(0);
    sum ^= black_box(to_in_nearest_i32(&kibi)).unwrap_or(0) as u32;
    sum ^= black_box(to_in_wrapping(&t0)).unwrap_or(0);
    sum ^= black_box(rescaled(&volt)).is_some() as u32;
    sum ^= black_box(volt < full_scale) as u32;
    sum ^= black_box(ordered(&t0, &t1)) as u32;
    sum ^= black_box(bounded_decode(in_range)) as u32;
    sum ^= !black_box(bounded_decode(out_of_range)) as u32;
    sum ^= black_box(to_fixed(&t0)).unwrap_or(0);
    sum ^= black_box(timed_roundtrip(&mut tbuf, &timed)) as u32;
    sum ^= black_box(steps(&t0)).unwrap_or(0);

    black_box(sum);
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}

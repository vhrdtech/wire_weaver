//! Panics of three kinds, each reached through a call chain the symbolizer has to name (CLT-59).

#![no_std]
#![no_main]

use core::hint::black_box;
use core::panic::PanicInfo;

/// `Option::unwrap`, inlined here, calls `core::option::unwrap_failed`.
#[inline(never)]
fn ratio(a: u32, b: u32) -> u32 {
    a.checked_div(b).unwrap()
}

/// Indexing past the end calls `core::panicking::panic_bounds_check`.
#[inline(never)]
fn pick(table: &[u8], i: usize) -> u8 {
    table[i]
}

/// Inlined into [scaled]: its `panic!` shows as an inlined frame inside `scaled`.
#[inline(always)]
fn check_gain(gain: u32) {
    if gain > 100 {
        panic!("gain too high");
    }
}

#[inline(never)]
fn scaled(x: u32, gain: u32) -> u32 {
    check_gain(gain);
    x.wrapping_mul(gain)
}

#[unsafe(no_mangle)]
pub extern "C" fn Reset() -> ! {
    let table = [1u8, 2, 3];
    let a = ratio(black_box(10), black_box(0));
    let b = pick(black_box(&table), black_box(5));
    let c = scaled(black_box(a), black_box(u32::from(b)));
    black_box(c);
    loop {}
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        black_box(());
    }
}

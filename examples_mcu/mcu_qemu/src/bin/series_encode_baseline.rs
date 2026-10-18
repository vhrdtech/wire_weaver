//! Baseline of the `Series<Timed<Ampere>>` encode bins: the same items and printing, no `Series`, Cortex-M3 (TEST-23).
#![no_main]
#![no_std]

extern crate panic_semihosting;

#[path = "../bench.rs"]
mod bench;
#[path = "../motor_signal.rs"]
mod motor_signal;

use cortex_m_rt::entry;

#[entry]
fn main() -> ! {
    bench::baseline()
}

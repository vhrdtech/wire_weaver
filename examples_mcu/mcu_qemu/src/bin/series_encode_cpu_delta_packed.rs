//! Series<Timed<Ampere>> encode, `DeltaPacked`, delta and packed residuals, XOR floats fixed at compile time (only those codecs linked), Cortex-M3 (TEST-23).
#![no_main]
#![no_std]

extern crate panic_semihosting;

#[path = "../bench.rs"]
mod bench;
#[path = "../motor_signal.rs"]
mod motor_signal;

use cortex_m_rt::entry;
use wire_weaver::shrink_wrap::series::DeltaPacked;

#[entry]
fn main() -> ! {
    bench::run("DeltaPacked", DeltaPacked)
}

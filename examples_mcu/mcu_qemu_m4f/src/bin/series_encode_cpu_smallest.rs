//! Series<Timed<Ampere>> encode, CodecPolicy::SMALLEST (tries every codec), Cortex-M4F (TEST-23).
#![no_main]
#![no_std]

extern crate panic_semihosting;

#[path = "../bench.rs"]
mod bench;
#[path = "../motor_signal.rs"]
mod motor_signal;

use cortex_m_rt::entry;
use wire_weaver::shrink_wrap::series::CodecPolicy;

#[entry]
fn main() -> ! {
    bench::run("SMALLEST", CodecPolicy::SMALLEST)
}

//! Series<Timed<Ampere>> encode, CodecPolicy::fixed(Delta, Packed), Cortex-M3 (TEST-23).
#![no_main]
#![no_std]

extern crate panic_semihosting;

#[path = "../bench.rs"]
mod bench;
#[path = "../motor_signal.rs"]
mod motor_signal;

use cortex_m_rt::entry;
use wire_weaver::shrink_wrap::series::{CodecPolicy, Coder, FloatCodec, NumericCodec, Predictor};

#[entry]
fn main() -> ! {
    bench::run(
        "fixed(delta,packed)",
        CodecPolicy::fixed(
            NumericCodec::new(Predictor::Delta, Coder::Packed),
            FloatCodec::Plain,
        ),
    )
}

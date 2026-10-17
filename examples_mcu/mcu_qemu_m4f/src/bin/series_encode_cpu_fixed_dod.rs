//! Series<Timed<Ampere>> encode, CodecPolicy::fixed(DeltaOfDelta, Packed), Cortex-M4F (TEST-23).
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
        "fixed(delta-of-delta,packed)",
        CodecPolicy::fixed(
            NumericCodec::new(Predictor::DeltaOfDelta, Coder::Packed),
            FloatCodec::Plain,
        ),
    )
}

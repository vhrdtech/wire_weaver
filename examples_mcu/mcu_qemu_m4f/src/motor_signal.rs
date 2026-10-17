//! 64 consecutive FOC-tick `i_a` samples (phase current, milliamps) of `examples/motor_demo`'s real simulator:
//! `Device::new(Options::default())` (preset `VescOutrunner`, profile `SpeedSteps`, seed 1, default `Fan` load),
//! `motor.truth().i_a` quantized `Ampere::quantize(.., Prefix::Milli, NumericBaseType::I32)` at each 40 us PWM
//! tick, starting right after the profile's 8000 -> -4000 erpm step at simulated t = 1.0 s (a current reversal,
//! the sharpest transient in the recorded run). TEST-23, series-encode-cpu.
//!
//! Regenerate with a scratch `examples/motor_demo/examples/scratch_dump_ia.rs` driving `Device::step()` (private
//! fields, so it must live in a crate that can see them; delete it again after copying the printed arrays here).

/// Device time of the first sample, simulated microseconds.
pub const T0_US: u32 = 1_000_040;
/// One PWM period.
pub const DT_US: u32 = 40;

pub const I_A_MILLIAMPS: [i32; 64] = [
    -243, -193, -235, -248, -238, -247, -264, -295, -269, -252, -326, -344, -343, -336, -284, -362,
    -369, -314, -303, -335, -369, -391, -367, -388, -384, 2098, 4284, 6186, 7892, 9381, 10708,
    11841, 12874, 13781, 14582, 15265, 15873, 16416, 16888, 17345, 17733, 18107, 18404, 18652,
    18902, 19135, 19296, 19514, 19684, 19784, 19948, 20118, 20196, 20282, 20350, 20442, 20483,
    20538, 20571, 20531, 20481, 20425, 20342, 20307,
];

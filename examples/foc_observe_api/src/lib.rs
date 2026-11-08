//! The 25 kHz FOC level of the motor demo (motor_sim's `motor_demo_api::Foc`) as an API root of its own: the
//! 21 `ro+observe+batched` properties, same names and types, for the `examples_mcu` benches of observe cells
//! (API-50) against `<Level>Sampler::sample`. No methods: the benches sample and poll from the firmware.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
use ww_si::{Ampere, Radian, Ratio, Volt, Weber};

#[ww_api_root]
pub trait Foc {
    /// Phase a current: `Milli` `I32`.
    property!(ro+observe+batched i_a: Ampere);
    /// Phase b current: `Milli` `I32`.
    property!(ro+observe+batched i_b: Ampere);
    /// Phase c current: `Milli` `I32`.
    property!(ro+observe+batched i_c: Ampere);
    /// α current (Clarke): `Milli` `I32`.
    property!(ro+observe+batched i_alpha: Ampere);
    /// β current: `Milli` `I32`.
    property!(ro+observe+batched i_beta: Ampere);
    /// d current (Park at `phase`): `Milli` `I32`.
    property!(ro+observe+batched i_d: Ampere);
    /// q current: `Milli` `I32`.
    property!(ro+observe+batched i_q: Ampere);
    /// d voltage the current loop asks for: `Milli` `I32`.
    property!(ro+observe+batched v_d: Volt);
    /// q voltage: `Milli` `I32`.
    property!(ro+observe+batched v_q: Volt);
    /// α voltage: `Milli` `I32`.
    property!(ro+observe+batched v_alpha: Volt);
    /// β voltage: `Milli` `I32`.
    property!(ro+observe+batched v_beta: Volt);
    /// Leg a duty, 0..=1: `Micro` `I32`.
    property!(ro+observe+batched duty_a: Ratio);
    /// Leg b duty: `Micro` `I32`.
    property!(ro+observe+batched duty_b: Ratio);
    /// Leg c duty: `Micro` `I32`.
    property!(ro+observe+batched duty_c: Ratio);
    /// Electrical angle the current loop uses, [0, 2π): `Micro` `I32`.
    property!(ro+observe+batched phase: Radian);
    /// Electrical angle from the flux observer: `Micro` `I32`.
    property!(ro+observe+batched phase_observer: Radian);
    /// Electrical angle from the encoder: `Micro` `I32`.
    property!(ro+observe+batched phase_encoder: Radian);
    /// Flux observer state α: `Nano` `I32`.
    property!(ro+observe+batched observer_x1: Weber);
    /// Flux observer state β: `Nano` `I32`.
    property!(ro+observe+batched observer_x2: Weber);
    /// DC link voltage as measured: `Milli` `I32`.
    property!(ro+observe+batched v_bus: Volt);
    /// True electrical angle (simulator): `Micro` `I32`.
    property!(ro+observe+batched phase_true: Radian);
}

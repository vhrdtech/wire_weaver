//! API of the motor controller demo (EX-11): a VESC-like FOC controller driving a simulated motor, with its ~50
//! variables as `+observe` properties.
//!
//! The variables are split into trait levels by the rate they change at, like the three tiers of the VESC firmware
//! (vedderb/bldc): [Foc] is the 25 kHz current loop (`mcpwm_foc` ISR), [Control] the 1 kHz speed loop and filtered
//! values, [Telemetry] the totals VESC Tool polls at 50 Hz (`COMM_GET_VALUES`). Observing a level sends one
//! `PropertyUpdate` per level carrying the properties that are due, so a host asks for each tier at its own rate.
//! [Sim] sets up the simulation: speed of simulated time, load, motor, faults.
//!
//! The device's clock is simulated time: observe periods, timestamps and budgets are in simulated microseconds and
//! bytes per simulated second.
//!
//! Values are `ww_si` quantities. The device picks prefix and storage per property, as each one's docs say: `I32` at
//! a fixed prefix for signed physical values (the prefix sets the resolution, noise below it costs no bits), `U32`
//! for totals that only grow, `F32` for values that span decades. Setpoints and settings the host writes may come in
//! any of the three. A firmware of this API enables only those number types (`ww_si` features `i32`, `u32`, `f32`).
#![no_std]

use wire_weaver::prelude::*;
use ww_si::{
    Ampere, AngularVelocity, Coulomb, Degree, Joule, Radian, Ratio, Second, Torque, Volt, Watt,
    Weber,
};

#[ww_api_root]
pub trait MotorDemo {
    /// 25 kHz tier: current loop, observer and PWM, one value per PWM period.
    ww_impl!(foc: Foc);
    /// 1 kHz tier: setpoints, speed loop and filtered values.
    ww_impl!(control: Control);
    /// 50 Hz tier: counters, totals and status.
    ww_impl!(telemetry: Telemetry);
    /// Simulation settings.
    ww_impl!(sim: Sim);
}

/// The current loop, run every PWM period (40 µs).
#[ww_trait]
pub trait Foc {
    /// Phase a current as measured (offset calibrated): `Milli` `I32`.
    property!(ro+observe i_a: Ampere);
    /// Phase b current: `Milli` `I32`.
    property!(ro+observe i_b: Ampere);
    /// Phase c current: `Milli` `I32`.
    property!(ro+observe i_c: Ampere);
    /// α current (Clarke): `Milli` `I32`.
    property!(ro+observe i_alpha: Ampere);
    /// β current: `Milli` `I32`.
    property!(ro+observe i_beta: Ampere);
    /// d current (Park at `phase`): `Milli` `I32`.
    property!(ro+observe i_d: Ampere);
    /// q current: `Milli` `I32`.
    property!(ro+observe i_q: Ampere);
    /// d voltage the current loop asks for: `Milli` `I32`.
    property!(ro+observe v_d: Volt);
    /// q voltage: `Milli` `I32`.
    property!(ro+observe v_q: Volt);
    /// α voltage: `Milli` `I32`.
    property!(ro+observe v_alpha: Volt);
    /// β voltage: `Milli` `I32`.
    property!(ro+observe v_beta: Volt);
    /// Leg a duty, 0..=1: `Micro` `I32` (PWM resolution is ~2.4e-4, `Milli` would lose it).
    property!(ro+observe duty_a: Ratio);
    /// Leg b duty: `Micro` `I32`.
    property!(ro+observe duty_b: Ratio);
    /// Leg c duty: `Micro` `I32`.
    property!(ro+observe duty_c: Ratio);
    /// Electrical angle the current loop uses, [0, 2π): `Micro` `I32`. Wraps once per electrical turn: one full
    /// value per turn in a `Series` (200 per second at 12 000 erpm against 25 000 samples).
    property!(ro+observe phase: Radian);
    /// Electrical angle from the flux observer: `Micro` `I32`.
    property!(ro+observe phase_observer: Radian);
    /// Electrical angle from the encoder: `Micro` `I32`.
    property!(ro+observe phase_encoder: Radian);
    /// Flux observer state α: `Nano` `I32` (flux linkage is a few mWb).
    property!(ro+observe observer_x1: Weber);
    /// Flux observer state β: `Nano` `I32`.
    property!(ro+observe observer_x2: Weber);
    /// DC link voltage as measured: `Milli` `I32`.
    property!(ro+observe v_bus: Volt);
    /// True electrical angle (simulator), what a real controller can't see: `Micro` `I32`.
    property!(ro+observe phase_true: Radian);
}

/// Setpoints and the 1 kHz loop.
#[ww_trait]
pub trait Control {
    /// What the controller follows. Setting it clears a fault.
    property!(rw+observe mode: ControlMode);
    /// Where the current loop gets its angle from.
    property!(rw+observe sensor_mode: SensorMode);
    /// Duty in [ControlMode::Duty], -0.95..=0.95: any storage the host sends (F32 from Python is fine).
    property!(rw+observe duty_target: Ratio);
    /// q current in [ControlMode::Current].
    property!(rw+observe current_target: Ampere);
    /// Electrical speed in [ControlMode::Speed] (VESC's erpm is this x 60 / 2π).
    property!(rw+observe speed_target: AngularVelocity);
    /// Electrical speed from the PLL: `Milli` `I32` (12 000 erpm = 1 257 rad/s).
    property!(ro+observe speed_electrical: AngularVelocity);
    /// Mechanical speed: `Milli` `I32`.
    property!(ro+observe speed: AngularVelocity);
    /// d current setpoint: `Milli` `I32`.
    property!(ro+observe id_set: Ampere);
    /// q current setpoint: `Milli` `I32`.
    property!(ro+observe iq_set: Ampere);
    /// d current, low-pass filtered: `Milli` `I32`.
    property!(ro+observe id_filtered: Ampere);
    /// q current, low-pass filtered: `Milli` `I32`.
    property!(ro+observe iq_filtered: Ampere);
    /// Motor current (q current, filtered): `Milli` `I32`.
    property!(ro+observe i_motor: Ampere);
    /// DC link (battery) current, estimated: `Milli` `I32`.
    property!(ro+observe i_input: Ampere);
    /// Duty now, |v_dq| over the most the inverter gives, signed by v_q: `Micro` `I32`.
    property!(ro+observe duty_now: Ratio);
    /// Torque estimated from the q current: `Milli` `I32`.
    property!(ro+observe torque: Torque);
    /// Rotor position from the encoder, [0, 360): `Milli` `I32`.
    property!(ro+observe position: Degree);
    /// Observer angle minus encoder angle, electrical, (-180, 180]: `Milli` `I32`.
    property!(ro+observe observer_error: Degree);
    /// True mechanical speed (simulator): `Milli` `I32`.
    property!(ro+observe speed_true: AngularVelocity);
}

/// Counters and totals, as VESC Tool shows them.
#[ww_trait]
pub trait Telemetry {
    /// Simulated time since the start: `Milli` `U32` (wraps after 49.7 days).
    property!(ro+observe time: Second);
    /// Charge drawn from the battery: `Unit` `U32` (1 C = 0.28 mAh; `Milli` would wrap at 1.2 Ah). An integer total,
    /// not an f32 one, which stops counting small steps once it is big.
    property!(ro+observe charge_drawn: Coulomb);
    /// Charge returned to the battery: `Unit` `U32`.
    property!(ro+observe charge_returned: Coulomb);
    /// Energy drawn from the battery: `Unit` `U32` (1 J = 0.28 mWh).
    property!(ro+observe energy_drawn: Joule);
    /// Energy returned to the battery: `Unit` `U32`.
    property!(ro+observe energy_returned: Joule);
    /// Electrical sixths of a turn, signed (VESC's tachometer). A count, not a quantity.
    property!(ro+observe tachometer: i32);
    /// Electrical sixths of a turn, both directions.
    property!(ro+observe tachometer_abs: u32);
    /// DC link voltage, filtered: `Milli` `I32`.
    property!(ro+observe v_in: Volt);
    /// Input power: `Centi` `I32` (±21 kW; `Milli` would end at 2.1 kW, under 60 A x 48 V).
    property!(ro+observe power_in: Watt);
    /// Motor current averaged since the previous 50 Hz tick: `Milli` `I32`.
    property!(ro+observe avg_motor_current: Ampere);
    /// Input current averaged since the previous 50 Hz tick: `Milli` `I32`.
    property!(ro+observe avg_input_current: Ampere);
    /// Latched fault, cleared by setting `control.mode`.
    property!(ro+observe fault: Fault);
    /// Simulated seconds per real second, achieved: `F32`.
    property!(ro+observe real_time_factor: Ratio);
}

/// Simulation settings. Changing the motor or calling `reset` starts over from rest.
#[ww_trait]
pub trait Sim {
    /// Simulated seconds per real second asked for: 1 is real time, 0.01 a hundred times slower.
    property!(rw time_scale: Ratio);
    /// Load on the shaft.
    property!(rw load: Load);
    /// Motor and inverter.
    property!(rw motor: Preset);
    /// Built-in speed steps the controller follows when on. Writing `control.mode` or a target turns it off.
    property!(rw profile: Profile);
    /// Break or repair the sensor and inverter paths.
    fn inject(fault: InjectFault);
    /// Back to rest: currents, speed, totals and faults cleared, settings kept.
    fn reset();
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy), ww_repr = u4)]
pub enum ControlMode {
    /// Inverter off, the motor coasts.
    Off,
    /// Fixed duty (voltage) along q.
    Duty,
    /// q current.
    Current,
    /// Speed loop to `speed_target`.
    Speed,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy), ww_repr = u4)]
pub enum SensorMode {
    /// Encoder angle.
    Encoder,
    /// Flux observer angle (sensorless), from standstill too: may fail to start under load.
    Observer,
    /// Encoder below 2000 erpm, observer above.
    Hybrid,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy), ww_repr = u4)]
pub enum Fault {
    None,
    /// A phase current went over the absolute maximum, the inverter is off.
    OverCurrent,
    /// DC link reading above 130 % of nominal.
    OverVoltage,
    /// DC link reading below 70 % of nominal.
    UnderVoltage,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Clone, Copy), ww_repr = u4)]
pub enum Load {
    /// Free shaft.
    None,
    /// Torque = b·ω, N·m·s/rad: a coefficient without a ww_si quantity, stays f32.
    Viscous { b: f32 },
    /// Propeller: torque = k·ω·|ω|, N·m·s²/rad²: stays f32.
    Fan { k: f32 },
    /// Constant torque.
    Constant { torque: Torque },
    /// Shaft held still.
    Locked,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy), ww_repr = u4)]
pub enum Preset {
    /// VESC's default hobby outrunner on 3 cells.
    VescOutrunner,
    /// Small 48 V industrial servo.
    IndustrialSpm,
    /// Interior magnet motor.
    Ipm,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy), ww_repr = u4)]
pub enum Profile {
    /// Setpoints only change when written.
    Off,
    /// Speed mode, stepping through 2000, 8000, -4000 and 12000 erpm, half a second each.
    SpeedSteps,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Clone, Copy), ww_repr = u4)]
pub enum InjectFault {
    /// Repair everything.
    Clear,
    /// Extra offset on one phase current reading (0..=2).
    CurrentOffset { phase: u8, offset: Ampere },
    /// Chance per sample that the encoder misses a count.
    EncoderMissedCounts { probability: Ratio },
    /// Chance per step that a leg's PWM pulse is missed.
    MissedPulses { probability: Ratio },
    /// Extra offset on the DC link voltage reading: trips the over or under voltage fault when big enough.
    BusVoltageOffset { offset: Volt },
}

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
//! bytes per simulated second. Units are SI unless the name says otherwise (`erpm`, `deg`).
#![no_std]

use wire_weaver::prelude::*;

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
    /// Phase a current as measured (offset calibrated), A.
    property!(ro+observe i_a: f32);
    /// Phase b current, A.
    property!(ro+observe i_b: f32);
    /// Phase c current, A.
    property!(ro+observe i_c: f32);
    /// α current (Clarke), A.
    property!(ro+observe i_alpha: f32);
    /// β current, A.
    property!(ro+observe i_beta: f32);
    /// d current (Park at `phase`), A.
    property!(ro+observe i_d: f32);
    /// q current, A.
    property!(ro+observe i_q: f32);
    /// d voltage the current loop asks for, V.
    property!(ro+observe v_d: f32);
    /// q voltage, V.
    property!(ro+observe v_q: f32);
    /// α voltage, V.
    property!(ro+observe v_alpha: f32);
    /// β voltage, V.
    property!(ro+observe v_beta: f32);
    /// Leg a duty, 0..=1.
    property!(ro+observe duty_a: f32);
    /// Leg b duty, 0..=1.
    property!(ro+observe duty_b: f32);
    /// Leg c duty, 0..=1.
    property!(ro+observe duty_c: f32);
    /// Electrical angle the current loop uses, rad in [0, 2π).
    property!(ro+observe phase: f32);
    /// Electrical angle from the flux observer, rad.
    property!(ro+observe phase_observer: f32);
    /// Electrical angle from the encoder, rad.
    property!(ro+observe phase_encoder: f32);
    /// Flux observer state α, Wb.
    property!(ro+observe observer_x1: f32);
    /// Flux observer state β, Wb.
    property!(ro+observe observer_x2: f32);
    /// DC link voltage as measured, V.
    property!(ro+observe v_bus: f32);
    /// True electrical angle (simulator), rad: what a real controller can't see.
    property!(ro+observe phase_true: f32);
}

/// Setpoints and the 1 kHz loop.
#[ww_trait]
pub trait Control {
    /// What the controller follows. Setting it clears a fault.
    property!(rw+observe mode: ControlMode);
    /// Where the current loop gets its angle from.
    property!(rw+observe sensor_mode: SensorMode);
    /// Duty in [ControlMode::Duty], -0.95..=0.95.
    property!(rw+observe duty_target: f32);
    /// q current in [ControlMode::Current], A.
    property!(rw+observe current_target: f32);
    /// Speed in [ControlMode::Speed], electrical rpm.
    property!(rw+observe erpm_target: f32);
    /// Electrical speed from the PLL, rpm.
    property!(ro+observe erpm: f32);
    /// Mechanical speed, rpm.
    property!(ro+observe rpm: f32);
    /// d current setpoint, A.
    property!(ro+observe id_set: f32);
    /// q current setpoint, A.
    property!(ro+observe iq_set: f32);
    /// d current, low-pass filtered, A.
    property!(ro+observe id_filtered: f32);
    /// q current, low-pass filtered, A.
    property!(ro+observe iq_filtered: f32);
    /// Motor current (q current, filtered), A.
    property!(ro+observe i_motor: f32);
    /// DC link (battery) current, estimated from the voltages and currents, A.
    property!(ro+observe i_input: f32);
    /// Duty now: |v_dq| over the most the inverter gives, signed by v_q.
    property!(ro+observe duty_now: f32);
    /// Torque estimated from the q current, N·m.
    property!(ro+observe torque: f32);
    /// Rotor position from the encoder, degrees in [0, 360).
    property!(ro+observe position_deg: f32);
    /// Observer angle minus encoder angle, electrical degrees in (-180, 180].
    property!(ro+observe observer_error_deg: f32);
    /// True mechanical speed (simulator), rpm.
    property!(ro+observe rpm_true: f32);
}

/// Counters and totals, as VESC Tool shows them.
#[ww_trait]
pub trait Telemetry {
    /// Simulated time since the start, s.
    property!(ro+observe time_s: f32);
    /// Charge drawn from the battery, Ah.
    property!(ro+observe amp_hours: f32);
    /// Charge returned to the battery, Ah.
    property!(ro+observe amp_hours_charged: f32);
    /// Energy drawn from the battery, Wh.
    property!(ro+observe watt_hours: f32);
    /// Energy returned to the battery, Wh.
    property!(ro+observe watt_hours_charged: f32);
    /// Electrical sixths of a turn, signed (VESC's tachometer).
    property!(ro+observe tachometer: i32);
    /// Electrical sixths of a turn, both directions.
    property!(ro+observe tachometer_abs: u32);
    /// DC link voltage, filtered, V.
    property!(ro+observe v_in: f32);
    /// Input power, W.
    property!(ro+observe power_in: f32);
    /// Motor current averaged since the previous 50 Hz tick, A.
    property!(ro+observe avg_motor_current: f32);
    /// Input current averaged since the previous 50 Hz tick, A.
    property!(ro+observe avg_input_current: f32);
    /// Latched fault, cleared by setting `control.mode`.
    property!(ro+observe fault: Fault);
    /// Simulated seconds per real second, achieved.
    property!(ro+observe real_time_factor: f32);
}

/// Simulation settings. Changing the motor or calling `reset` starts over from rest.
#[ww_trait]
pub trait Sim {
    /// Simulated seconds per real second asked for: 1 is real time, 0.01 a hundred times slower.
    property!(rw time_scale: f32);
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
    /// Speed loop to `erpm_target`.
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
    /// Torque = b·ω, N·m·s/rad.
    Viscous { b: f32 },
    /// Propeller: torque = k·ω·|ω|, N·m·s²/rad².
    Fan { k: f32 },
    /// Constant torque, N·m.
    Constant { torque: f32 },
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
    /// Extra offset on one phase current reading (0..=2), A.
    CurrentOffset { phase: u8, amps: f32 },
    /// Chance per sample that the encoder misses a count.
    EncoderMissedCounts { probability: f32 },
    /// Chance per step that a leg's PWM pulse is missed.
    MissedPulses { probability: f32 },
    /// Extra offset on the DC link voltage reading, V: trips the over or under voltage fault when big enough.
    BusVoltageOffset { volts: f32 },
}

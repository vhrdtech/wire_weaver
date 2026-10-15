//! A VESC-like field oriented controller (after vedderb/bldc `mcpwm_foc.c`), in three tiers:
//!
//! - every PWM period ([Controller::pwm_step], 25 kHz): current offsets, Clarke and Park, the Ortega flux observer
//!   and PLL, d/q current PI loops with decoupling, space vector PWM;
//! - every 25th period (1 kHz): speed PI loop, filtered values, input current, torque;
//! - every 500th period (50 Hz): averages and the Ah/Wh counters.
//!
//! Gains come from the motor parameters, as VESC's motor detection would set them. The controller only reads
//! [motor_sim::Sensors]; the true state is only reported next to it (`phase_true`, `rpm_true`).

use core::f32::consts::{PI, TAU};

use motor_demo_api::{ControlMode, Fault, SensorMode};
use motor_sim::{Drive, MotorParams, Sensors, Truth};

/// PWM frequency, Hz.
pub const PWM_HZ: u32 = 25_000;
/// PWM periods per control loop run (1 kHz).
pub const CONTROL_DIV: u32 = 25;
/// PWM periods per telemetry run (50 Hz).
pub const TELEMETRY_DIV: u32 = 500;
/// PWM periods spent measuring the current offsets after a reset, with the inverter off (0.1 s).
pub const CALIBRATION_PERIODS: u32 = 2_500;
/// Hybrid sensor mode: observer above this speed, electrical rpm.
pub const HYBRID_ERPM: f32 = 2_000.0;

const SQRT3: f32 = 1.732_050_8;
const ONE_BY_SQRT3: f32 = 0.577_350_26;
const TWO_BY_SQRT3: f32 = 1.154_700_5;

/// Gains and limits.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub pole_pairs: f32,
    pub rs: f32,
    pub ld: f32,
    pub lq: f32,
    pub lambda: f32,
    /// Nominal DC link, V.
    pub vdc: f32,
    pub current_kp: f32,
    pub current_ki: f32,
    /// Ortega observer gain γ.
    pub observer_gain: f32,
    pub pll_kp: f32,
    pub pll_ki: f32,
    /// A per erpm.
    pub speed_kp: f32,
    /// A per erpm·s.
    pub speed_ki: f32,
    /// q current limit, A.
    pub current_max: f32,
    /// Phase current that trips [Fault::OverCurrent], A.
    pub current_abs_max: f32,
    pub encoder_cpr: u32,
    /// Encoder zero relative to the d axis, mechanical rad.
    pub encoder_offset: f32,
}

impl Config {
    /// Tuned from the motor: current loops with a 0.2 ms time constant (pole-zero cancellation), a 10 Hz speed loop,
    /// observer gain for a 1000 rad/s radial bandwidth. The PLL is faster than VESC's default (kp 2000, ki 30 000):
    /// the light outrunner accelerates at ~60 000 rad/s² electrical, which would make that one slip.
    /// `current_max` is the drive's limit.
    pub fn from_params(p: &MotorParams, current_max: f32) -> Self {
        let tc = 0.2e-3;
        let pp = p.pmsm.pole_pairs as f32;
        let lambda = p.pmsm.flux_linkage_wb;
        let l = 0.5 * (p.pmsm.ld_h + p.pmsm.lq_h);
        let kt = 1.5 * pp * lambda;
        // erpm per second per amp of q current
        let accel = kt / p.mech.inertia_kgm2 * 60.0 / TAU * pp;
        let bw = TAU * 10.0;
        let speed_kp = bw / accel;
        Config {
            pole_pairs: pp,
            rs: p.pmsm.rs_ohm,
            ld: p.pmsm.ld_h,
            lq: p.pmsm.lq_h,
            lambda,
            vdc: p.inverter.vdc_v,
            current_kp: l / tc,
            current_ki: p.pmsm.rs_ohm / tc,
            observer_gain: 1_000.0 / (lambda * lambda),
            pll_kp: 4_000.0,
            pll_ki: 4_000_000.0,
            speed_kp,
            speed_ki: speed_kp * bw / 4.0,
            current_max,
            current_abs_max: (current_max * 2.0).min(p.sensors.current_full_scale_a * 0.95),
            encoder_cpr: p.sensors.encoder_cpr,
            encoder_offset: p.sensors.encoder_offset_rad,
        }
    }
}

/// Setpoints, written by the host.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Setpoints {
    pub mode: ControlMode,
    pub sensor_mode: SensorMode,
    pub duty: f32,
    pub current: f32,
    pub erpm: f32,
}

impl Default for Setpoints {
    fn default() -> Self {
        Setpoints {
            mode: ControlMode::Off,
            sensor_mode: SensorMode::Encoder,
            duty: 0.0,
            current: 0.0,
            erpm: 0.0,
        }
    }
}

/// 25 kHz values, updated every PWM period.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FocValues {
    pub i_a: f32,
    pub i_b: f32,
    pub i_c: f32,
    pub i_alpha: f32,
    pub i_beta: f32,
    pub i_d: f32,
    pub i_q: f32,
    pub v_d: f32,
    pub v_q: f32,
    pub v_alpha: f32,
    pub v_beta: f32,
    pub duty_a: f32,
    pub duty_b: f32,
    pub duty_c: f32,
    pub phase: f32,
    pub phase_observer: f32,
    pub phase_encoder: f32,
    pub observer_x1: f32,
    pub observer_x2: f32,
    pub v_bus: f32,
    pub phase_true: f32,
}

/// 1 kHz values.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ControlValues {
    pub erpm: f32,
    pub rpm: f32,
    pub id_set: f32,
    pub iq_set: f32,
    pub id_filtered: f32,
    pub iq_filtered: f32,
    pub i_motor: f32,
    pub i_input: f32,
    pub duty_now: f32,
    pub torque: f32,
    pub position_deg: f32,
    pub observer_error_deg: f32,
    pub rpm_true: f32,
}

/// 50 Hz values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TelemetryValues {
    pub time_s: f32,
    pub amp_hours: f32,
    pub amp_hours_charged: f32,
    pub watt_hours: f32,
    pub watt_hours_charged: f32,
    pub tachometer: i32,
    pub tachometer_abs: u32,
    pub v_in: f32,
    pub power_in: f32,
    pub avg_motor_current: f32,
    pub avg_input_current: f32,
    pub fault: Fault,
}

impl Default for TelemetryValues {
    fn default() -> Self {
        TelemetryValues {
            time_s: 0.0,
            amp_hours: 0.0,
            amp_hours_charged: 0.0,
            watt_hours: 0.0,
            watt_hours_charged: 0.0,
            tachometer: 0,
            tachometer_abs: 0,
            v_in: 0.0,
            power_in: 0.0,
            avg_motor_current: 0.0,
            avg_input_current: 0.0,
            fault: Fault::None,
        }
    }
}

/// Which tiers ran in a [Controller::pwm_step].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ran {
    pub control: bool,
    pub telemetry: bool,
}

/// The controller. Owns no motor: [pwm_step](Self::pwm_step) takes the sensors and returns the drive.
pub struct Controller {
    pub cfg: Config,
    pub set: Setpoints,
    pub foc: FocValues,
    pub control: ControlValues,
    pub telemetry: TelemetryValues,
    periods: u32,
    dt: f32,
    // offsets
    offset_sum: [f32; 3],
    offsets: [f32; 3],
    // observer and PLL
    x1: f32,
    x2: f32,
    pll_phase: f32,
    pll_speed: f32,
    // current and speed loops
    int_d: f32,
    int_q: f32,
    int_speed: f32,
    // tachometer
    sector: i32,
    // 1 kHz and 50 Hz accumulators (f64: they grow)
    sum_i_motor: f64,
    sum_i_input: f64,
    sum_n: u32,
    ah: f64,
    ah_charged: f64,
    wh: f64,
    wh_charged: f64,
    v_in_filtered: f32,
}

impl Controller {
    pub fn new(cfg: Config) -> Self {
        Controller {
            cfg,
            set: Setpoints::default(),
            foc: FocValues::default(),
            control: ControlValues::default(),
            telemetry: TelemetryValues::default(),
            periods: 0,
            dt: 1.0 / PWM_HZ as f32,
            offset_sum: [0.0; 3],
            offsets: [0.0; 3],
            x1: cfg.lambda,
            x2: 0.0,
            pll_phase: 0.0,
            pll_speed: 0.0,
            int_d: 0.0,
            int_q: 0.0,
            int_speed: 0.0,
            sector: 0,
            sum_i_motor: 0.0,
            sum_i_input: 0.0,
            sum_n: 0,
            ah: 0.0,
            ah_charged: 0.0,
            wh: 0.0,
            wh_charged: 0.0,
            v_in_filtered: cfg.vdc,
        }
    }

    /// Still measuring the current offsets after a reset, the inverter is off.
    pub fn calibrating(&self) -> bool {
        self.periods < CALIBRATION_PERIODS
    }

    /// Clear a fault and stop the loops' integrators, e.g. when the mode is set.
    pub fn clear_fault(&mut self) {
        self.telemetry.fault = Fault::None;
        self.int_d = 0.0;
        self.int_q = 0.0;
        self.int_speed = 0.0;
    }

    /// One PWM period: sensors taken at its start, the drive to hold until the next one. `truth` is only copied into
    /// the `*_true` values.
    pub fn pwm_step(&mut self, s: &Sensors, truth: &Truth) -> (Drive, Ran) {
        let cfg = self.cfg;
        let dt = self.dt;
        self.periods = self.periods.saturating_add(1);
        if self.calibrating() {
            for k in 0..3 {
                self.offset_sum[k] += s.i_abc_a[k];
            }
            if !self.calibrating() {
                self.offsets = self.offset_sum.map(|x| x / CALIBRATION_PERIODS as f32);
            }
        }
        let calibrating = self.calibrating();
        let f = &mut self.foc;
        f.i_a = s.i_abc_a[0] - self.offsets[0];
        f.i_b = s.i_abc_a[1] - self.offsets[1];
        f.i_c = s.i_abc_a[2] - self.offsets[2];
        f.v_bus = s.vdc_v;
        f.phase_true = truth.theta_e;
        // Clarke from a and b, as VESC does with two shunts
        f.i_alpha = f.i_a;
        f.i_beta = ONE_BY_SQRT3 * f.i_a + TWO_BY_SQRT3 * f.i_b;

        // Ortega flux observer on the voltage applied over the last period
        let (ia, ib) = (f.i_alpha, f.i_beta);
        let l = 0.5 * (cfg.ld + cfg.lq);
        let (lia, lib) = (l * ia, l * ib);
        let err = cfg.lambda * cfg.lambda
            - ((self.x1 - lia) * (self.x1 - lia) + (self.x2 - lib) * (self.x2 - lib));
        let g = 0.5 * cfg.observer_gain * err;
        self.x1 += (f.v_alpha - cfg.rs * ia + g * (self.x1 - lia)) * dt;
        self.x2 += (f.v_beta - cfg.rs * ib + g * (self.x2 - lib)) * dt;
        f.observer_x1 = self.x1;
        f.observer_x2 = self.x2;
        f.phase_observer = wrap_tau((self.x2 - lib).atan2(self.x1 - lia));

        let mech =
            s.encoder_count as f32 * TAU / cfg.encoder_cpr.max(1) as f32 - cfg.encoder_offset;
        f.phase_encoder = wrap_tau(cfg.pole_pairs * mech);

        let use_observer = match self.set.sensor_mode {
            SensorMode::Encoder | SensorMode::Unknown(_) => false,
            SensorMode::Observer => true,
            SensorMode::Hybrid => self.control.erpm.abs() > HYBRID_ERPM,
        };
        f.phase = if use_observer {
            f.phase_observer
        } else {
            f.phase_encoder
        };

        // PLL on the angle in use
        let delta = wrap_pi(f.phase - self.pll_phase);
        self.pll_phase = wrap_tau(self.pll_phase + (self.pll_speed + cfg.pll_kp * delta) * dt);
        self.pll_speed += cfg.pll_ki * delta * dt;

        let (sin, cos) = (f.phase.sin(), f.phase.cos());
        f.i_d = cos * ia + sin * ib;
        f.i_q = cos * ib - sin * ia;

        let over = [f.i_a, f.i_b, f.i_c]
            .iter()
            .any(|i| i.abs() > cfg.current_abs_max);
        if !calibrating && self.telemetry.fault == Fault::None {
            if over {
                self.telemetry.fault = Fault::OverCurrent;
            } else if s.vdc_v > 1.3 * cfg.vdc {
                self.telemetry.fault = Fault::OverVoltage;
            } else if s.vdc_v < 0.7 * cfg.vdc {
                self.telemetry.fault = Fault::UnderVoltage;
            }
        }

        let v_max = s.vdc_v.max(1.0) * ONE_BY_SQRT3 * 0.95;
        let off =
            calibrating || self.telemetry.fault != Fault::None || self.set.mode == ControlMode::Off;
        let drive = if off {
            self.int_d = 0.0;
            self.int_q = 0.0;
            f.v_d = 0.0;
            f.v_q = 0.0;
            f.v_alpha = 0.0;
            f.v_beta = 0.0;
            f.duty_a = 0.0;
            f.duty_b = 0.0;
            f.duty_c = 0.0;
            Drive::Off
        } else {
            let omega = self.pll_speed;
            let (id_set, iq_set) = (self.control.id_set, self.control.iq_set);
            let ed = id_set - f.i_d;
            let eq = iq_set - f.i_q;
            self.int_d = (self.int_d + cfg.current_ki * ed * dt).clamp(-v_max, v_max);
            self.int_q = (self.int_q + cfg.current_ki * eq * dt).clamp(-v_max, v_max);
            let mut vd = cfg.current_kp * ed + self.int_d - omega * cfg.lq * f.i_q;
            let mut vq = if self.set.mode == ControlMode::Duty {
                self.int_q = 0.0;
                self.set.duty.clamp(-0.95, 0.95) * v_max
            } else {
                cfg.current_kp * eq + self.int_q + omega * (cfg.ld * f.i_d + cfg.lambda)
            };
            // limit |v|, d first as VESC does
            vd = vd.clamp(-v_max, v_max);
            let vq_max = (v_max * v_max - vd * vd).sqrt();
            vq = vq.clamp(-vq_max, vq_max);
            f.v_d = vd;
            f.v_q = vq;
            // angle advanced half a period: the voltage acts over the next one
            let p = f.phase + 0.5 * omega * dt;
            let (sin, cos) = (p.sin(), p.cos());
            f.v_alpha = cos * vd - sin * vq;
            f.v_beta = sin * vd + cos * vq;
            let d = svpwm(f.v_alpha, f.v_beta, s.vdc_v.max(1.0));
            [f.duty_a, f.duty_b, f.duty_c] = d;
            Drive::Duty(d)
        };

        // per period accumulators
        let i_input = 1.5 * (f.v_d * f.i_d + f.v_q * f.i_q) / s.vdc_v.max(1.0);
        self.sum_i_motor += f.i_q as f64;
        self.sum_i_input += i_input as f64;
        self.sum_n += 1;
        let ah = i_input as f64 * dt as f64 / 3600.0;
        if ah >= 0.0 {
            self.ah += ah;
            self.wh += ah * s.vdc_v as f64;
        } else {
            self.ah_charged -= ah;
            self.wh_charged -= ah * s.vdc_v as f64;
        }
        let sector = (self.pll_phase / (PI / 3.0)) as i32;
        let step = (sector - self.sector).rem_euclid(6);
        if step == 1 {
            self.telemetry.tachometer += 1;
            self.telemetry.tachometer_abs += 1;
        } else if step == 5 {
            self.telemetry.tachometer -= 1;
            self.telemetry.tachometer_abs += 1;
        }
        self.sector = sector;
        self.v_in_filtered += (s.vdc_v - self.v_in_filtered) * 0.001;

        let mut ran = Ran::default();
        if self.periods.is_multiple_of(CONTROL_DIV) {
            self.control_step(i_input, v_max, truth);
            ran.control = true;
        }
        if self.periods.is_multiple_of(TELEMETRY_DIV) {
            self.telemetry_step(truth);
            ran.telemetry = true;
        }
        (drive, ran)
    }

    fn control_step(&mut self, i_input: f32, v_max: f32, truth: &Truth) {
        let cfg = self.cfg;
        let dt = CONTROL_DIV as f32 * self.dt;
        let f = &self.foc;
        let c = &mut self.control;
        c.erpm = self.pll_speed * 60.0 / TAU;
        c.rpm = c.erpm / cfg.pole_pairs;
        c.rpm_true = truth.omega_m * 60.0 / TAU;
        c.id_set = 0.0;
        c.iq_set = match self.set.mode {
            ControlMode::Off | ControlMode::Duty | ControlMode::Unknown(_) => {
                self.int_speed = 0.0;
                0.0
            }
            ControlMode::Current => self.set.current,
            ControlMode::Speed => {
                let e = self.set.erpm - c.erpm;
                self.int_speed = (self.int_speed + cfg.speed_ki * e * dt)
                    .clamp(-cfg.current_max, cfg.current_max);
                cfg.speed_kp * e + self.int_speed
            }
        }
        .clamp(-cfg.current_max, cfg.current_max);
        // ~10 ms filter, as VESC's motor current filter at 1 kHz
        let a = 0.1;
        c.id_filtered += (f.i_d - c.id_filtered) * a;
        c.iq_filtered += (f.i_q - c.iq_filtered) * a;
        c.i_motor = c.iq_filtered;
        c.i_input = i_input;
        let v = (f.v_d * f.v_d + f.v_q * f.v_q).sqrt();
        c.duty_now = (v / v_max).copysign(f.v_q);
        c.torque = 1.5 * cfg.pole_pairs * (cfg.lambda + (cfg.ld - cfg.lq) * f.i_d) * f.i_q;
        c.position_deg = wrap_tau(f.phase_encoder / cfg.pole_pairs) * 360.0 / TAU;
        c.observer_error_deg = wrap_pi(f.phase_observer - f.phase_encoder) * 360.0 / TAU;
    }

    fn telemetry_step(&mut self, truth: &Truth) {
        let t = &mut self.telemetry;
        t.time_s = truth.time_s as f32;
        t.amp_hours = self.ah as f32;
        t.amp_hours_charged = self.ah_charged as f32;
        t.watt_hours = self.wh as f32;
        t.watt_hours_charged = self.wh_charged as f32;
        t.v_in = self.v_in_filtered;
        let n = self.sum_n.max(1) as f64;
        t.avg_motor_current = (self.sum_i_motor / n) as f32;
        t.avg_input_current = (self.sum_i_input / n) as f32;
        t.power_in = t.avg_input_current * t.v_in;
        self.sum_i_motor = 0.0;
        self.sum_i_input = 0.0;
        self.sum_n = 0;
    }
}

/// Space vector PWM (min-max zero sequence): leg duties for an αβ voltage.
fn svpwm(v_alpha: f32, v_beta: f32, vdc: f32) -> [f32; 3] {
    let va = v_alpha;
    let vb = -0.5 * v_alpha + 0.5 * SQRT3 * v_beta;
    let vc = -0.5 * v_alpha - 0.5 * SQRT3 * v_beta;
    let zero = -0.5 * (va.max(vb).max(vc) + va.min(vb).min(vc));
    [va, vb, vc].map(|v| (0.5 + (v + zero) / vdc).clamp(0.0, 1.0))
}

/// `x` in [0, 2π).
pub fn wrap_tau(x: f32) -> f32 {
    let r = x % TAU;
    if r < 0.0 { r + TAU } else { r }
}

/// `x` in (-π, π].
pub fn wrap_pi(x: f32) -> f32 {
    let r = wrap_tau(x);
    if r > PI { r - TAU } else { r }
}

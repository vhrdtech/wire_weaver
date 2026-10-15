//! The controller against the simulated motor, without wire_weaver: current, speed and duty control, the observer
//! and PLL, every motor preset.

use motor_demo::controller::{Config, Controller, PWM_HZ};
use motor_demo_api::{ControlMode, SensorMode};
use motor_sim::{Load, Motor, MotorParams, presets};

struct Bench {
    motor: Motor,
    ctl: Controller,
}

impl Bench {
    fn new(params: MotorParams, current_max: f32, load: Load) -> Self {
        let mut motor = Motor::new(params, 7);
        motor.set_load(load);
        Bench {
            motor,
            ctl: Controller::new(Config::from_params(&params, current_max)),
        }
    }

    fn run(&mut self, seconds: f32) {
        let dt = 1.0 / PWM_HZ as f32;
        for _ in 0..(seconds * PWM_HZ as f32) as u32 {
            let (drive, _) = self.ctl.pwm_step(self.motor.sensors(), self.motor.truth());
            self.motor.step(&drive, dt);
        }
    }

    fn erpm_true(&self) -> f32 {
        self.motor.truth().omega_e * 60.0 / core::f32::consts::TAU
    }
}

fn vesc(load: Load) -> Bench {
    Bench::new(presets::vesc_hobby_outrunner(), 60.0, load)
}

#[test]
fn current_mode_follows_q_current() {
    let mut b = vesc(Load::Viscous { b_nms_per_rad: 5e-4 });
    b.run(0.2); // offsets calibrated
    assert!(!b.ctl.calibrating());
    b.ctl.set.mode = ControlMode::Current;
    b.ctl.set.current = 10.0;
    b.run(0.05);
    let iq = b.motor.truth().i_q;
    assert!((iq - 10.0).abs() < 0.5, "true iq {iq}");
    assert!(b.motor.truth().i_d.abs() < 0.5, "true id {}", b.motor.truth().i_d);
    assert!(b.erpm_true() > 1000.0, "{}", b.erpm_true());
}

#[test]
fn speed_mode_reaches_setpoint() {
    let mut b = vesc(Load::Fan { k_nms2_per_rad2: 2e-7 });
    b.run(0.2);
    b.ctl.set.mode = ControlMode::Speed;
    b.ctl.set.erpm = 8_000.0;
    b.run(0.5);
    let e = b.erpm_true();
    assert!((e - 8_000.0).abs() < 80.0, "{e}");
    assert!((b.ctl.control.erpm - 8_000.0).abs() < 80.0, "{:?}", b.ctl.control);
    // reverse
    b.ctl.set.erpm = -4_000.0;
    b.run(0.5);
    let e = b.erpm_true();
    assert!((e + 4_000.0).abs() < 80.0, "{e}");
}

#[test]
fn observer_tracks_the_angle_at_speed() {
    let mut b = vesc(Load::Fan { k_nms2_per_rad2: 2e-7 });
    b.run(0.2);
    b.ctl.set.mode = ControlMode::Speed;
    b.ctl.set.erpm = 10_000.0;
    b.run(0.5);
    let err = b.ctl.control.observer_error_deg;
    assert!(err.abs() < 5.0, "observer error {err}°");
    // sensorless from here on
    b.ctl.set.sensor_mode = SensorMode::Observer;
    b.ctl.set.erpm = 15_000.0;
    b.run(0.5);
    let e = b.erpm_true();
    assert!((e - 15_000.0).abs() < 150.0, "{e}");
}

#[test]
fn hybrid_starts_from_standstill() {
    let mut b = vesc(Load::Viscous { b_nms_per_rad: 1e-4 });
    b.run(0.2);
    b.ctl.set.sensor_mode = SensorMode::Hybrid;
    b.ctl.set.mode = ControlMode::Speed;
    b.ctl.set.erpm = 12_000.0;
    b.run(0.6);
    let e = b.erpm_true();
    assert!((e - 12_000.0).abs() < 120.0, "{e}");
}

#[test]
fn duty_mode_spins() {
    let mut b = vesc(Load::Viscous { b_nms_per_rad: 1e-4 });
    b.run(0.2);
    b.ctl.set.mode = ControlMode::Duty;
    b.ctl.set.duty = 0.3;
    b.run(0.5);
    assert!(b.erpm_true() > 5_000.0, "{}", b.erpm_true());
    assert!((b.ctl.control.duty_now - 0.3).abs() < 0.02, "{:?}", b.ctl.control);
}

#[test]
fn every_preset_follows_speed() {
    for (params, imax) in [
        (presets::vesc_hobby_outrunner(), 60.0),
        (presets::industrial_spm(), 10.0),
        (presets::ipm(), 40.0),
    ] {
        let mut b = Bench::new(params, imax, Load::Viscous { b_nms_per_rad: 1e-4 });
        b.run(0.2);
        b.ctl.set.mode = ControlMode::Speed;
        b.ctl.set.erpm = 6_000.0;
        b.run(1.0);
        let e = b.erpm_true();
        assert!((e - 6_000.0).abs() < 60.0, "{params:?}: {e}");
    }
}

#[test]
fn counters_count() {
    let mut b = vesc(Load::Viscous { b_nms_per_rad: 5e-4 });
    b.run(0.2);
    b.ctl.set.mode = ControlMode::Speed;
    b.ctl.set.erpm = 6_000.0;
    b.run(1.0);
    let t = b.ctl.telemetry;
    // 6000 erpm = 100 electrical turns per second, 6 counts each, less the spin-up
    assert!((300..=600).contains(&t.tachometer), "{t:?}");
    assert_eq!(t.tachometer_abs, t.tachometer as u32);
    let drawn = b.motor.truth().energy_drawn_wh as f32;
    assert!(
        (t.watt_hours - drawn).abs() < 0.1 * drawn,
        "{} vs {drawn}",
        t.watt_hours
    );
    assert!((t.time_s - 1.2).abs() < 0.03, "{}", t.time_s);
}

//! The data sets: where each comes from and how its float and fixed-point forms are made.

use anyhow::Context;
use motor_sim::{Drive, Load, Motor, Value, presets};
use std::path::Path;

/// Columns of one scalar type, each a whole variable with its channels interleaved.
pub enum Columns {
    F32(Vec<Vec<f32>>),
    F64(Vec<Vec<f64>>),
    I16(Vec<Vec<i16>>),
    I32(Vec<Vec<i32>>),
}

/// A data set: its floats and the same quantity as fixed-point integers.
pub struct DataSet {
    pub name: String,
    /// Channels interleaved in every column.
    pub channels: u32,
    pub floats: Columns,
    pub fixed: Columns,
    /// What the fixed-point form is.
    pub fixed_note: &'static str,
}

/// Simulated seconds of the motor data set.
const MOTOR_SECONDS: usize = 1;
/// The PWM rate the motor variables are sampled at.
const MOTOR_RATE_HZ: usize = 25_000;
/// Seed of the simulation's sensor noise.
const MOTOR_SEED: u64 = 42;

/// Every variable of `motor_sim`'s VESC preset at 25 kHz, as `f64` (`F32` variables widened, `U32` as numbers):
/// a fan load, the drive turning the stator voltage ahead of the rotor angle, its amplitude ramping up and down.
fn motor_vars() -> (Vec<&'static str>, Vec<Vec<f64>>) {
    let mut motor = Motor::new(presets::vesc_hobby_outrunner(), MOTOR_SEED);
    motor.set_load(Load::Fan {
        k_nms2_per_rad2: 2e-7,
    });
    let steps = MOTOR_SECONDS * MOTOR_RATE_HZ;
    let dt = 1.0 / MOTOR_RATE_HZ as f32;
    let mut names = vec![];
    let mut vars: Vec<Vec<f64>> = vec![];
    for i in 0..steps {
        let t = i as f32 / steps as f32;
        let amplitude = 6.0 * libm::sinf(core::f32::consts::PI * t);
        let theta = motor.truth().theta_e;
        let drive = Drive::AlphaBeta {
            v_alpha_v: -amplitude * libm::sinf(theta),
            v_beta_v: amplitude * libm::cosf(theta),
        };
        motor.step(&drive, dt);
        let mut k = 0;
        motor.visit(&mut |var: &motor_sim::Var, value: Value| {
            let v = match value {
                Value::F32(x) => f64::from(x),
                Value::F64(x) => x,
                Value::U32(x) => f64::from(x),
            };
            if i == 0 {
                names.push(var.name);
                vars.push(Vec::with_capacity(steps));
            }
            if let Some(col) = vars.get_mut(k) {
                col.push(v);
            }
            k += 1;
        });
    }
    (names, vars)
}

/// Each variable as `i32` at the finest power-of-ten step that keeps its peak within 24 bits, the precision of an
/// `f32` at full scale.
fn fixed_of(col: &[f64]) -> Vec<i32> {
    let peak = col.iter().fold(0f64, |m, x| m.max(x.abs()));
    let mut step = 1e-12;
    while peak / step > f64::from(1u32 << 23) {
        step *= 10.0;
    }
    col.iter().map(|x| (x / step).round() as i32).collect()
}

/// The motor data set as `f32` and as `f64`.
pub fn motor() -> [DataSet; 2] {
    let (_, vars) = motor_vars();
    let f32s = vars
        .iter()
        .map(|c| c.iter().map(|x| *x as f32).collect())
        .collect();
    let fixed: Vec<Vec<i32>> = vars.iter().map(|c| fixed_of(c)).collect();
    let note = "i32, power-of-ten step keeping 24 bits at the peak";
    [
        DataSet {
            name: "motor_sim f32".into(),
            channels: 1,
            floats: Columns::F32(f32s),
            fixed: Columns::I32(fixed.clone()),
            fixed_note: note,
        },
        DataSet {
            name: "motor_sim f64".into(),
            channels: 1,
            floats: Columns::F64(vars),
            fixed: Columns::I32(fixed),
            fixed_note: note,
        },
    ]
}

/// The speech recording decoded to `f32` by lewton, and lewton's own 16-bit PCM of it as the fixed-point form.
pub fn speech(path: &Path) -> anyhow::Result<DataSet> {
    let open = || -> anyhow::Result<_> {
        let file = std::fs::File::open(path).with_context(|| format!("{}", path.display()))?;
        Ok(lewton::inside_ogg::OggStreamReader::new(
            std::io::BufReader::new(file),
        )?)
    };
    let mut r = open()?;
    let channels = u32::from(r.ident_hdr.audio_channels);
    let mut floats = vec![];
    while let Some(p) = r.read_dec_packet_generic::<lewton::samples::InterleavedSamples<f32>>()? {
        floats.extend(p.samples);
    }
    let mut r = open()?;
    let mut pcm = vec![];
    while let Some(p) = r.read_dec_packet_itl()? {
        pcm.extend(p);
    }
    anyhow::ensure!(
        pcm.len() == floats.len(),
        "the two decodes differ in length"
    );
    Ok(DataSet {
        name: format!("speech f32, {channels} ch"),
        channels,
        floats: Columns::F32(vec![floats]),
        fixed: Columns::I16(vec![pcm]),
        fixed_note: "i16 PCM of the same decode",
    })
}

/// Samples per channel of the ADC data sets.
const ADC_SAMPLES: usize = 250_000;
/// ADC channels, interleaved.
const ADC_CHANNELS: usize = 4;

/// xorshift64*.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
}

/// 12-bit ADC counts of 4 slow channels with noise of a few counts, interleaved.
fn adc_counts() -> Vec<i16> {
    let mut r = Rng(7);
    let mut out = Vec::with_capacity(ADC_SAMPLES * ADC_CHANNELS);
    for i in 0..ADC_SAMPLES {
        for c in 0..ADC_CHANNELS {
            let period = 5000.0 * (c + 1) as f64;
            let wave = 1200.0 * libm::sin(2.0 * core::f64::consts::PI * i as f64 / period);
            let noise = (r.next() >> 61) as f64 - 3.5;
            out.push((2048.0 + wave + noise).round().clamp(0.0, 4095.0) as i16);
        }
    }
    out
}

/// ADC counts turned into floats by firmware: `counts × scale` in the float type.
pub fn adc() -> [DataSet; 4] {
    let counts = adc_counts();
    let volts = 3.3 / 4096.0;
    let make = |name: &str, floats| DataSet {
        name: name.into(),
        channels: ADC_CHANNELS as u32,
        floats,
        fixed: Columns::I16(vec![counts.clone()]),
        fixed_note: "i16 ADC counts",
    };
    [
        make(
            "ADC x 3.3/4096 f32",
            Columns::F32(vec![
                counts
                    .iter()
                    .map(|c| f32::from(*c) * (3.3f32 / 4096.0))
                    .collect(),
            ]),
        ),
        make(
            "ADC x 3.3/4096 f64",
            Columns::F64(vec![counts.iter().map(|c| f64::from(*c) * volts).collect()]),
        ),
        make(
            "ADC x 0.1 f32",
            Columns::F32(vec![counts.iter().map(|c| f32::from(*c) * 0.1).collect()]),
        ),
        make(
            "ADC x 0.1 f64",
            Columns::F64(vec![counts.iter().map(|c| f64::from(*c) * 0.1).collect()]),
        ),
    ]
}

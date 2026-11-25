//! A board that exists only in software, for the simulated device and the tests.

use core::time::Duration;

use thermometer_api::{Gain, IndicationMode, Temperature};
use ww_device::Instant;

use crate::{Board, Input, Reading};

/// The room the simulated inputs sit in: 21.50 °C, in steps of 0.01 K.
const ROOM: i16 = 2150;
/// Each input reads 0.25 °C warmer than the one before it.
const INPUT_STEP: i16 = 25;
/// The room drifts up and down by 1.00 °C ...
const DRIFT: i16 = 100;
/// ... once a minute.
const DRIFT_PERIOD: Duration = Duration::from_secs(60);
/// Above this gain the simulated amplifier clips.
const GAIN_CLIPS_ABOVE: Gain = Gain::bounded(8);
/// A triggered conversion takes 100 ms.
const CONVERSION: Duration = Duration::from_millis(100);

/// A board with four inputs in a room whose temperature drifts slowly, the same at every run: a reading depends
/// only on the input and the time. Above gain 8 its amplifier clips.
#[derive(Debug, Clone)]
pub struct SimBoard {
    indication: IndicationMode,
    inputs: Input,
    /// The room keeps 21.50 °C: a reading depends only on the input.
    still: bool,
}

impl Default for SimBoard {
    fn default() -> Self {
        SimBoard {
            indication: IndicationMode::Normal,
            inputs: Input::bounded(4),
            still: false,
        }
    }
}

impl SimBoard {
    /// A board in a room that keeps 21.50 °C, so that every run reads the same temperatures at any time: for a
    /// script whose output must not change.
    pub fn still() -> Self {
        SimBoard {
            still: true,
            ..SimBoard::default()
        }
    }

    /// What the status LED shows.
    pub fn indication(&self) -> IndicationMode {
        self.indication
    }

    /// The temperature `input` reads at `now`.
    pub fn temperature(input: Input, now: Instant) -> Temperature {
        let period_us = DRIFT_PERIOD.as_micros() as u64;
        let phase = now.as_micros() % period_us;
        let half = period_us / 2;
        // a triangle from -DRIFT to +DRIFT and back over one period
        let rising = if phase < half {
            phase
        } else {
            period_us - phase
        };
        let drift = (rising * 4 * DRIFT as u64 / period_us) as i16 - DRIFT;
        let offset = (input.get() as i16 - 1) * INPUT_STEP;
        Temperature::from_raw(ROOM + offset + drift)
    }
}

impl Board for SimBoard {
    fn inputs(&self) -> Input {
        self.inputs
    }

    fn conversion_time(&self) -> Duration {
        CONVERSION
    }

    fn read(&mut self, input: Input, gain: Gain, now: Instant) -> Reading {
        let at = if self.still {
            // the drift's midpoint, where the room is at ROOM
            Instant::from_micros(DRIFT_PERIOD.as_micros() as u64 / 4)
        } else {
            now
        };
        Reading {
            temp: Self::temperature(input, at),
            overload: gain.get() > GAIN_CLIPS_ABOVE.get(),
        }
    }

    fn indicate(&mut self, mode: IndicationMode) {
        self.indication = mode;
    }
}

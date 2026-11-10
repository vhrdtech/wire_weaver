//! Line 1 (`thermo_api` 1.x, wire w1.y): the generated client and its operations.

use crate::error::{DeviceError, Error, Result};
use crate::model::{self, Gain, Offset, Rate, Uptime};
use crate::{Commands, Updates};
use thermo_api_w1 as api;
use wire_weaver_client::facade::{LineClient, ObserverOf, SinkOf, StreamOf};
use wire_weaver_client::observe::on_change;

wire_weaver_client::facade_line!(thermo_api_w1::Thermo);
// The API crate's own types: one conversion each way per type, also for the ones that didn't change (`RateError`,
// `Command`). The `ww_si` fields inside them are the model's types already.

fn sample(s: api::Sample) -> Result<model::Sample> {
    Ok(model::Sample {
        seq: s.seq,
        input: s.input,
        // 0.01 K in an i16 always fits 0.001 K in an i32
        temp: s.temp.to().expect("centi-kelvin fits milli-kelvin in i32"),
    })
}

fn mode(m: api::Mode) -> Result<model::Mode> {
    Ok(match m {
        api::Mode::Idle => model::Mode::Idle,
        api::Mode::Continuous => model::Mode::Continuous,
        api::Mode::Triggered => model::Mode::Triggered,
        api::Mode::Unknown(code) => return Err(Error::UnknownVariant { what: "Mode", code }),
    })
}

fn to_mode(m: model::Mode) -> api::Mode {
    match m {
        model::Mode::Idle => api::Mode::Idle,
        model::Mode::Continuous => api::Mode::Continuous,
        model::Mode::Triggered => api::Mode::Triggered,
    }
}

fn gain(
    g: ww_si::Fixed<ww_si::units::Ratio, ww_si::prefix::One, u8, ww_si::Range<1, 16>>,
) -> Result<Gain> {
    // 1..=16 whole steps are 100..=1600 hundredths: in range by construction
    Ok(g.to::<ww_si::prefix::Centi, u16>()
        .and_then(|g| g.to_bounds().ok())
        .expect("line-1 gain fits the model's"))
}

/// Whole seconds in a `u32` always fit milliseconds in a `u64`.
fn uptime(s: ww_si::Fixed<ww_si::units::Second, ww_si::prefix::One, u32>) -> Uptime {
    s.to().expect("seconds in u32 fit milliseconds in u64")
}

fn command(c: model::Command) -> Result<api::Command> {
    Ok(match c {
        model::Command::Trigger => api::Command::Trigger,
        model::Command::Mark { seq } => api::Command::Mark { seq },
    })
}

impl From<DeviceError<api::RateError>> for Error {
    fn from(e: DeviceError<api::RateError>) -> Self {
        match e.0 {
            api::RateError::Busy => Error::Busy,
            api::RateError::Unknown(code) => Error::UnknownVariant {
                what: "RateError",
                code,
            },
        }
    }
}

fn not_representable(c: &Client, what: &'static str, value: impl core::fmt::Debug) -> Error {
    Error::NotRepresentable {
        what,
        value: format!("{value:?}"),
        device: c.wire(),
    }
}

impl crate::Line for Client {
    /// Line 1 has no `configure`: two calls, not atomic, the rate first so that a refused rate leaves the mode alone.
    async fn configure(c: &Client, rate: Rate, mode: model::Mode) -> Result<()> {
        c.set_rate(rate).call().await?;
        Ok(c.write_mode(to_mode(mode)).write().await?)
    }

    async fn read_once(c: &Client) -> Result<model::Sample> {
        sample(c.read_once().call().await?)
    }

    async fn mode(c: &Client) -> Result<model::Mode> {
        mode(c.read_mode().read().await?)
    }

    async fn set_mode(c: &Client, m: model::Mode) -> Result<()> {
        Ok(c.write_mode(to_mode(m)).write().await?)
    }

    async fn gain(c: &Client) -> Result<Gain> {
        gain(c.read_gain().read().await?)
    }

    /// Whole steps only: 1.5 is refused, not rounded.
    async fn set_gain(c: &Client, g: Gain) -> Result<()> {
        let whole = g
            .to::<ww_si::prefix::One, u8>()
            .and_then(|g| g.to_bounds().ok())
            .ok_or_else(|| not_representable(c, "gain", g))?;
        Ok(c.write_gain(whole).write().await?)
    }

    async fn observe_gain(c: &Client) -> Result<Updates<Gain>> {
        let observer = c
            .observe_gain(on_change(Default::default()))
            .subscribe()
            .await?;
        Ok(Box::new(ObserverOf { observer, f: gain }))
    }

    async fn samples(c: &Client) -> Result<Updates<model::Sample>> {
        let stream = c.samples().call().await?;
        Ok(Box::new(StreamOf { stream, f: sample }))
    }

    async fn commands(c: &Client) -> Result<Commands> {
        let sink = c.commands().await?;
        Ok(Box::new(SinkOf { sink, f: command }))
    }

    async fn channel_enabled(c: &Client, ch: u32) -> Result<bool> {
        Ok(c.channel(ch).read_enabled().read().await?)
    }

    async fn set_channel_enabled(c: &Client, ch: u32, enabled: bool) -> Result<()> {
        Ok(c.channel(ch).write_enabled(enabled).write().await?)
    }

    async fn channel_last(c: &Client, ch: u32) -> Result<model::Sample> {
        sample(c.channel(ch).last().call().await?)
    }

    /// Line 1 takes 0.01 K steps: a finer offset is refused.
    async fn calibrate(c: &Client, offset: Offset) -> Result<()> {
        let centi = offset
            .to::<ww_si::prefix::Centi, i16>()
            .ok_or_else(|| not_representable(c, "calibrate offset", offset))?;
        c.calibrate(centi)
            .call()
            .await
            .map_err(|e| Error::from(e).of("calibrate"))
    }

    async fn uptime(c: &Client) -> Result<Uptime> {
        let s = c
            .read_uptime()
            .read()
            .await
            .map_err(|e| Error::from(e).of("uptime"))?;
        Ok(uptime(s))
    }

    // no self_test on line 1: `Unsupported` by default
}

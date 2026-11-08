//! Line 1 (`thermo_api` 1.x, wire w1.y): the generated client and its [Backend].

use crate::backend::{Backend, Commands, Fut, ObserverOf, SinkOf, StreamOf, Updates};
use crate::error::{DeviceError, Error, Result};
use crate::model::{self, Gain, Offset, Rate, Uptime};
use thermo_api_w1 as api;
use wire_weaver::ww_version::WireVersion;
use wire_weaver_client::observe::on_change;
use wire_weaver_client::{ClientConfig, Commander, WwClient};

pub struct Client {
    cmd: Commander,
}

impl WwClient for Client {
    fn default_config() -> ClientConfig {
        ClientConfig::default()
    }

    fn from_cmd(cmd: Commander) -> Self {
        Self { cmd }
    }
}

mod api_client {
    wire_weaver::ww_codegen!(
        thermo_api_w1 :: Thermo for super::Client,
        client = "std_client",
    );
}

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

impl Client {
    fn not_representable(&self, what: &'static str, value: impl core::fmt::Debug) -> Error {
        Error::NotRepresentable {
            what,
            value: format!("{value:?}"),
            device: self.wire(),
        }
    }
}

impl Backend for Client {
    fn cmd(&self) -> &Commander {
        &self.cmd
    }

    fn wire(&self) -> WireVersion {
        self.cmd
            .device_api_info()
            .user_api_wire
            .expect("link setup refuses a device without a wire version")
    }

    /// Line 1 has no `configure`: two calls, not atomic, the rate first so that a refused rate leaves the mode alone.
    fn configure(&self, rate: Rate, mode: model::Mode) -> Fut<'_, ()> {
        Box::pin(async move {
            self.set_rate(rate).call().await?;
            Ok(self.write_mode(to_mode(mode)).write().await?)
        })
    }

    fn read_once(&self) -> Fut<'_, model::Sample> {
        Box::pin(async move { sample(Client::read_once(self).call().await?) })
    }

    fn mode(&self) -> Fut<'_, model::Mode> {
        Box::pin(async move { mode(self.read_mode().read().await?) })
    }

    fn set_mode(&self, m: model::Mode) -> Fut<'_, ()> {
        Box::pin(async move { Ok(self.write_mode(to_mode(m)).write().await?) })
    }

    fn gain(&self) -> Fut<'_, Gain> {
        Box::pin(async move { gain(self.read_gain().read().await?) })
    }

    /// Whole steps only: 1.5 is refused, not rounded.
    fn set_gain(&self, g: Gain) -> Fut<'_, ()> {
        Box::pin(async move {
            let whole = g
                .to::<ww_si::prefix::One, u8>()
                .and_then(|g| g.to_bounds().ok())
                .ok_or_else(|| self.not_representable("gain", g))?;
            Ok(self.write_gain(whole).write().await?)
        })
    }

    fn observe_gain(&self) -> Fut<'_, Box<dyn Updates<Gain>>> {
        Box::pin(async move {
            let observer = Client::observe_gain(self, on_change(Default::default()))
                .subscribe()
                .await?;
            Ok(Box::new(ObserverOf { observer, f: gain }) as Box<dyn Updates<Gain>>)
        })
    }

    fn samples(&self) -> Fut<'_, Box<dyn Updates<model::Sample>>> {
        Box::pin(async move {
            let stream = Client::samples(self).call().await?;
            Ok(Box::new(StreamOf { stream, f: sample }) as Box<dyn Updates<model::Sample>>)
        })
    }

    fn commands(&self) -> Fut<'_, Box<dyn Commands>> {
        Box::pin(async move {
            let sink = Client::commands(self).await?;
            Ok(Box::new(SinkOf { sink, f: command }) as Box<dyn Commands>)
        })
    }

    fn channel_enabled(&self, ch: u32) -> Fut<'_, bool> {
        Box::pin(async move { Ok(self.channel(ch).read_enabled().read().await?) })
    }

    fn set_channel_enabled(&self, ch: u32, enabled: bool) -> Fut<'_, ()> {
        Box::pin(async move { Ok(self.channel(ch).write_enabled(enabled).write().await?) })
    }

    fn channel_last(&self, ch: u32) -> Fut<'_, model::Sample> {
        Box::pin(async move { sample(self.channel(ch).last().call().await?) })
    }

    /// Line 1 takes 0.01 K steps: a finer offset is refused.
    fn calibrate(&self, offset: Offset) -> Fut<'_, ()> {
        Box::pin(async move {
            let centi = offset
                .to::<ww_si::prefix::Centi, i16>()
                .ok_or_else(|| self.not_representable("calibrate offset", offset))?;
            Client::calibrate(self, centi)
                .call()
                .await
                .map_err(|e| Error::from(e).of("calibrate"))
        })
    }

    fn uptime(&self) -> Fut<'_, Uptime> {
        Box::pin(async move {
            let s = self
                .read_uptime()
                .read()
                .await
                .map_err(|e| Error::from(e).of("uptime"))?;
            Ok(uptime(s))
        })
    }

    fn self_test(&self) -> Fut<'_, ()> {
        Box::pin(async move {
            Err(Error::Unsupported {
                what: "self_test",
                device: self.wire(),
                needs: "line 2 from w2.1".into(),
            })
        })
    }
}

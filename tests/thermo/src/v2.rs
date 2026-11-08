//! Line 2 (`thermo_api` 2.x, wire w2.y): the generated client and its [Backend].

use crate::backend::{Backend, Commands, Fut, ObserverOf, SinkOf, StreamOf, Updates};
use crate::error::{DeviceError, Error, Result};
use crate::model::{self, Gain, Offset, Rate, Uptime};
use thermo_api_w2 as api;
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
        thermo_api_w2 :: Thermo for super::Client,
        client = "std_client",
    );
}

// The API crate's own types: one conversion each way per type, also for the ones that didn't change (`RateError`,
// `Command`). The `ww_si` fields inside them are the model's types already.

fn sample(s: api::Sample) -> Result<model::Sample> {
    Ok(model::Sample {
        seq: s.seq,
        input: s.input,
        temp: s.temp,
    })
}

fn mode(m: api::Mode) -> Result<model::Mode> {
    Ok(match m {
        api::Mode::Idle => model::Mode::Idle,
        api::Mode::Continuous => model::Mode::Continuous,
        api::Mode::Unknown(code) => return Err(Error::UnknownVariant { what: "Mode", code }),
    })
}

fn gain(g: Gain) -> Result<Gain> {
    Ok(g)
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

impl From<DeviceError<api::SelfTestError>> for Error {
    fn from(e: DeviceError<api::SelfTestError>) -> Self {
        Error::SelfTest(match e.0 {
            api::SelfTestError::SensorOpen => model::SelfTestError::SensorOpen,
            api::SelfTestError::SensorShorted => model::SelfTestError::SensorShorted,
            api::SelfTestError::Unknown(code) => {
                return Error::UnknownVariant {
                    what: "SelfTestError",
                    code,
                };
            }
        })
    }
}

impl Client {
    /// `Mode::Triggered` is gone from line 2.
    fn to_mode(&self, m: model::Mode) -> Result<api::Mode> {
        match m {
            model::Mode::Idle => Ok(api::Mode::Idle),
            model::Mode::Continuous => Ok(api::Mode::Continuous),
            model::Mode::Triggered => Err(Error::Unsupported {
                what: "Mode::Triggered",
                device: self.wire(),
                needs: "line 1 (line 2 triggers in any mode)".into(),
            }),
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

    fn configure(&self, rate: Rate, mode: model::Mode) -> Fut<'_, ()> {
        Box::pin(async move {
            let config = api::Config {
                rate,
                mode: self.to_mode(mode)?,
            };
            Ok(Client::configure(self, config).call().await?)
        })
    }

    fn read_once(&self) -> Fut<'_, model::Sample> {
        Box::pin(async move { sample(Client::read_once(self).call().await?) })
    }

    fn mode(&self) -> Fut<'_, model::Mode> {
        Box::pin(async move { mode(self.read_mode().read().await?) })
    }

    fn set_mode(&self, m: model::Mode) -> Fut<'_, ()> {
        Box::pin(async move { Ok(self.write_mode(self.to_mode(m)?).write().await?) })
    }

    fn gain(&self) -> Fut<'_, Gain> {
        Box::pin(async move { gain(self.read_gain().read().await?) })
    }

    fn set_gain(&self, g: Gain) -> Fut<'_, ()> {
        Box::pin(async move { Ok(self.write_gain(g).write().await?) })
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

    fn calibrate(&self, offset: Offset) -> Fut<'_, ()> {
        Box::pin(async move { Ok(Client::calibrate(self, offset).call().await?) })
    }

    fn uptime(&self) -> Fut<'_, Uptime> {
        Box::pin(async move { Ok(uptime(self.read_uptime().read().await?)) })
    }

    fn self_test(&self) -> Fut<'_, ()> {
        Box::pin(async move {
            Client::self_test(self)
                .call()
                .await
                .map_err(|e| Error::from(e).of("self_test"))
        })
    }
}

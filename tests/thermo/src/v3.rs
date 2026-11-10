//! Line 3 (`thermo_api` 3.x, wire w3.y): the generated client and its [Backend].

use crate::backend::{Backend, Commands, Fut, ObserverOf, SinkOf, SplitStreamOf, Updates};
use crate::error::{DeviceError, Error, Result};
use crate::model::{self, Gain, Offset, Rate, Uptime};
use thermo_api_w3 as api;
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

    fn try_from_cmd(cmd: Commander) -> core::result::Result<Self, wire_weaver_client::Error> {
        Ok(Self { cmd })
    }
}

mod api_client {
    wire_weaver::ww_codegen!(
        thermo_api_w3 :: Thermo for super::Client,
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

/// A batch is consecutive samples of one input, numbered from `first`.
fn batch(b: api::BatchOwned) -> Result<Vec<model::Sample>> {
    let mut seq = b.first;
    Ok(b.temps
        .into_iter()
        .map(|temp| {
            let s = model::Sample {
                seq,
                input: b.input,
                temp,
            };
            seq = ww_si::Seq::new(seq.get().wrapping_add(1));
            s
        })
        .collect())
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
    /// `Mode::Triggered` is gone since line 2.
    fn to_mode(&self, m: model::Mode) -> Result<api::Mode> {
        match m {
            model::Mode::Idle => Ok(api::Mode::Idle),
            model::Mode::Continuous => Ok(api::Mode::Continuous),
            model::Mode::Triggered => Err(Error::Unsupported {
                what: "Mode::Triggered",
                device: self.wire(),
                needs: "line 1 (lines 2 and 3 trigger in any mode)".into(),
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
            Ok(Box::new(SplitStreamOf::new(stream, batch)) as Box<dyn Updates<model::Sample>>)
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
        Box::pin(async move { Ok(self.health().read_uptime().read().await?) })
    }

    fn self_test(&self) -> Fut<'_, ()> {
        Box::pin(async move {
            self.health()
                .self_test()
                .call()
                .await
                .map_err(|e| Error::from(e).of("self_test"))
        })
    }
}

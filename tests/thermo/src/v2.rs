//! Line 2 (`thermo_api` 2.x, wire w2.y): the generated client and its operations.

use crate::error::{DeviceError, Error, Result};
use crate::model::{self, Gain, Offset, Rate, Uptime};
use thermo_api_w2 as api;
use wire_weaver_client::facade::{LineClient, ObserverOf, SinkOf, StreamOf};
use wire_weaver_client::observe::on_change;

wire_weaver_client::facade_line!(thermo_api_w2::Thermo);

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

// conversions that need the device's wire version
impl Client {
    /// `Mode::Triggered` is gone from line 2.
    fn to_mode(c: &Client, m: model::Mode) -> Result<api::Mode> {
        match m {
            model::Mode::Idle => Ok(api::Mode::Idle),
            model::Mode::Continuous => Ok(api::Mode::Continuous),
            model::Mode::Triggered => Err(Error::Unsupported {
                what: "Mode::Triggered",
                device: c.wire(),
                needs: "line 1 (line 2 triggers in any mode)".into(),
            }),
        }
    }
}

impl crate::line::V2 for Client {
    type GainUpdates = ObserverOf<api_client::thermo_api_thermo::ThermoUpdate, Gain, Gain, Error>;
    type Samples = StreamOf<api::Sample, model::Sample, Error>;
    type Commands = SinkOf<api::Command, model::Command, Error>;

    async fn read_once(c: &Client) -> Result<model::Sample> {
        sample(c.read_once().call().await?)
    }

    async fn mode(c: &Client) -> Result<model::Mode> {
        mode(c.read_mode().read().await?)
    }

    async fn set_mode(c: &Client, m: model::Mode) -> Result<()> {
        Ok(c.write_mode(Client::to_mode(c, m)?).write().await?)
    }

    async fn gain(c: &Client) -> Result<Gain> {
        gain(c.read_gain().read().await?)
    }

    async fn set_gain(c: &Client, g: Gain) -> Result<()> {
        Ok(c.write_gain(g).write().await?)
    }

    async fn observe_gain(c: &Client) -> Result<Self::GainUpdates> {
        let observer = c
            .observe_gain(on_change(Default::default()))
            .subscribe()
            .await?;
        Ok(ObserverOf { observer, f: gain })
    }

    async fn commands(c: &Client) -> Result<Self::Commands> {
        let sink = c.commands().await?;
        Ok(SinkOf { sink, f: command })
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

    async fn calibrate(c: &Client, offset: Offset) -> Result<()> {
        Ok(c.calibrate(offset).call().await?)
    }

    async fn configure(c: &Client, rate: Rate, mode: model::Mode) -> Result<()> {
        let config = api::Config {
            rate,
            mode: Client::to_mode(c, mode)?,
        };
        Ok(c.configure(config).call().await?)
    }

    async fn samples(c: &Client) -> Result<Self::Samples> {
        let stream = c.samples().call().await?;
        Ok(StreamOf { stream, f: sample })
    }

    async fn uptime(c: &Client) -> Result<Uptime> {
        Ok(uptime(c.read_uptime().read().await?))
    }

    async fn self_test(c: &Client) -> Result<()> {
        c.self_test()
            .call()
            .await
            .map_err(|e| Error::from(e).of("self_test"))
    }
}

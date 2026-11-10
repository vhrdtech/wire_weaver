//! Line 2 (`thermo_api::wire_v2`, wire w2.y): the generated client and its operations.

use crate::common::{command, mode, sample, to_mode};
use crate::error::{Error, Result};
use crate::model::{self, Gain, Offset, Rate, Uptime};
use thermo_api::common as api;
use wire_weaver_client::facade::{LineClient, ObserverOf, SinkOf, StreamOf};
use wire_weaver_client::observe::on_change;

wire_weaver_client::facade_line!(
    thermo_api::wire_v2::Thermo,
    sub_levels = [ww_indication::Indication => crate::IndicationClient],
);

// This line's types are all in `thermo_api::common`, converted in `crate::common`; what is left is the line's own
// numbers:

fn gain(g: Gain) -> Result<Gain> {
    Ok(g)
}

/// Whole seconds in a `u32` always fit milliseconds in a `u64`.
fn uptime(s: ww_si::Fixed<ww_si::units::Second, ww_si::prefix::One, u32>) -> Uptime {
    s.to().expect("seconds in u32 fit milliseconds in u64")
}

impl crate::line::V2 for Client {
    type GainUpdates =
        ObserverOf<api_client::thermo_api_wire_v2_thermo::ThermoUpdate, Gain, Gain, Error>;
    type Samples = StreamOf<api::Sample, model::Sample, Error>;
    type Commands = SinkOf<api::Command, model::Command, Error>;

    async fn read_once(c: &Client) -> Result<model::Sample> {
        sample(c.read_once().call().await?)
    }

    async fn mode(c: &Client) -> Result<model::Mode> {
        mode(c.read_mode().read().await?)
    }

    async fn set_mode(c: &Client, m: model::Mode) -> Result<()> {
        Ok(c.write_mode(to_mode(c.wire(), m)?).write().await?)
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
            mode: to_mode(c.wire(), mode)?,
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

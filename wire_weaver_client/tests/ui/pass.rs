//! Every form `facade!` takes: a call, a stream, a sink, `#[unsupported]` before or after the docs, another
//! attribute, an operation without a return type.
#[path = "support/lines.rs"]
mod support;
#[allow(unused_imports)]
use support::{Error, v1, v2};

wire_weaver_client::facade! {
    /// A device on line 1 or 2.
    pub enum Device {
        V2(v2::Client),
        V1(v1::Client),
    }

    pub mod line;

    error = Error;

    /// A call.
    async fn read(&self, channel: u8) -> u32;
    #[must_use = "the reading is the point"]
    /// Docs after another attribute.
    async fn read_twice(&self) -> (u32, u32);
    #[unsupported(V1, needs = "line 2")]
    /// Docs after `#[unsupported]`.
    async fn reset(&self);
    /// Docs before `#[unsupported]`, two lines named.
    #[unsupported(V1, V2)]
    async fn calibrate(&self, offset: i16,);
    async fn values(&self) -> stream Values<u32>;
    async fn commands(&self) -> sink Commands<u8>;
}

impl line::V2 for v2::Client {
    type Values = wire_weaver_client::facade::Lacking;
    type Commands = wire_weaver_client::facade::Lacking;
    async fn read(_c: &Self, channel: u8) -> Result<u32, Error> {
        Ok(channel.into())
    }
    async fn read_twice(_c: &Self) -> Result<(u32, u32), Error> {
        Ok((1, 2))
    }
    async fn reset(_c: &Self) -> Result<(), Error> {
        Ok(())
    }
    async fn values(_c: &Self) -> Result<Self::Values, Error> {
        unimplemented!()
    }
    async fn commands(_c: &Self) -> Result<Self::Commands, Error> {
        unimplemented!()
    }
}

// line 1 writes neither `reset` nor `calibrate`
impl line::V1 for v1::Client {
    type Values = wire_weaver_client::facade::Lacking;
    type Commands = wire_weaver_client::facade::Lacking;
    async fn read(_c: &Self, _channel: u8) -> Result<u32, Error> {
        Ok(0)
    }
    async fn read_twice(_c: &Self) -> Result<(u32, u32), Error> {
        Ok((0, 0))
    }
    async fn values(_c: &Self) -> Result<Self::Values, Error> {
        unimplemented!()
    }
    async fn commands(_c: &Self) -> Result<Self::Commands, Error> {
        unimplemented!()
    }
}

fn main() {}

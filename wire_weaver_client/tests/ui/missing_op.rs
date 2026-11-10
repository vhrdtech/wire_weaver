//! Line 1 doesn't write `read`, which no `#[unsupported]` names for it: a compile error.
#[path = "support/lines.rs"]
mod support;
#[allow(unused_imports)]
use support::{Error, v1, v2};

wire_weaver_client::facade! {
    pub enum Device {
        V2(v2::Client),
        V1(v1::Client),
    }

    pub mod line;

    error = Error;

    /// A call.
    async fn read(&self) -> u32;
    #[unsupported(V1)]
    async fn reset(&self);
}

impl line::V2 for v2::Client {
    async fn read(_c: &Self) -> Result<u32, Error> {
        Ok(2)
    }
    async fn reset(_c: &Self) -> Result<(), Error> {
        Ok(())
    }
}

impl line::V1 for v1::Client {}

fn main() {}

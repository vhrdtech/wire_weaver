//! `facade!` without its `mod line;`.
#[path = "support/lines.rs"]
mod support;
#[allow(unused_imports, reason = "the support items are not all used by every case")]
use support::{Error, v1, v2};

wire_weaver_client::facade! {
    pub enum Device {
        V2(v2::Client),
        V1(v1::Client),
    }

    error = Error;

    async fn read(&self) -> u32;
}

fn main() {}

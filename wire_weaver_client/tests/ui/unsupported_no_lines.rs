//! `#[unsupported]` naming no line.
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

    #[unsupported(needs = "line 2")]
    async fn reset(&self);
}



fn main() {}

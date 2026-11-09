//! Line 1.

use super::common::Code;
use wire_weaver::prelude::*;

#[ww_api_root]
pub trait Thermo {
    fn read_once() -> Code;
    ww_impl!(channel: super::common::Channel);
}

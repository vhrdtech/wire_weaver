//! Line 1: the channel and the indication on the root.

use super::common::Code;
use wire_weaver::prelude::*;

#[ww_api_root]
pub trait Thermo {
    fn read_once() -> Code;
    ww_impl!(channel: super::common::Channel);
    ww_impl!(indication: ww_indication::Indication);
}

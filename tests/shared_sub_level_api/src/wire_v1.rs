//! Line 1: the channel and the indication on the root.

use super::common::Code;
use ww_api::prelude::*;

#[ww_api_root]
#[effect(edit)]
pub trait Thermo {
    fn read_once() -> Code;
    ww_impl!(channel: super::common::Channel);
    ww_impl!(indication: super::common::Indication);
}

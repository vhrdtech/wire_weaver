//! Line 1.

use super::common::Code;
use ww_api::prelude::*;

#[ww_api_root]
pub trait Thermo {
    fn read_once() -> Code;
    ww_impl!(channel: super::common::Channel);
}

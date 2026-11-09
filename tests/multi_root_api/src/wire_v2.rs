//! Line 2: `read_once` takes the number of samples to average, a wire break.

use super::common::Code;
use wire_weaver::prelude::*;

#[ww_api_root]
pub trait Thermo {
    fn read_once(average: u8) -> Code;
    ww_impl!(channel: super::common::Channel);
}

//! Line 2: `read_once` takes the number of samples to average, a wire break. It names the shared sub-trait through a
//! `use`, line 1 by its path: both find `common::Channel`.

use super::common::{Channel, Code};
use wire_weaver::prelude::*;

#[ww_api_root]
pub trait Thermo {
    fn read_once(average: u8) -> Code;
    ww_impl!(channel: Channel);
}

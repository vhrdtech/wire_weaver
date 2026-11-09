//! Line 2: the channel moved one level down, under `probe`, and auxiliary channels as an array.

use super::common::Code;
use wire_weaver::prelude::*;

#[ww_api_root]
pub trait Thermo {
    fn read_once(average: u8) -> Code;
    ww_impl!(probe: Probe);
    ww_impl!(aux[]: super::common::Channel);
    ww_impl!(indication: ww_indication::Indication);
}

/// The probe, which has the channel now.
#[ww_trait]
pub trait Probe {
    ww_impl!(channel: super::common::Channel);
}

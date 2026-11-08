//! Test API root for `ww_pin_diag`, used by `tests/pin_diag`.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait PinDiagTest {
    ww_impl!(pins: ww_pin_diag::PinDiag);
}

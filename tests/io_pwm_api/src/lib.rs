//! Test API root for `ww_pwm`: every trait of the crate at one level, used by `tests/io_pwm`.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait IoPwmTest {
    ww_impl!(pwm: ww_pwm::Pwm);
    ww_impl!(pwm_capture: ww_pwm::PwmCapture);
}

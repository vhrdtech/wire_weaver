//! Test API root for `ww_adc`: every trait of the crate at one level, used by `tests/io_adc`.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait IoAdcTest {
    ww_impl!(adc: ww_adc::Adc);
    ww_impl!(adc_stream: ww_adc::AdcStream);
    ww_impl!(adc_capture: ww_adc::AdcCapture);
}

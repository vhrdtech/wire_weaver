//! Test API root for `ww_uart`: every trait of the crate at one level, used by `tests/io_uart`.
#![cfg_attr(not(feature = "std"), no_std)]

use ww_api::prelude::*;

#[ww_api_root]
pub trait IoUartTest {
    ww_impl!(uart: ww_uart::Uart);
}

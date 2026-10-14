//! Test API root for `ww_uart`: every trait of the crate at one level, used by `tests/io_uart`.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait IoUartTest {
    ww_impl!(uart: ww_uart::Uart);
    ww_impl!(uart_monitor: ww_uart::UartMonitor);
}

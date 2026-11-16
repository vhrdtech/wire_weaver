#![no_std]

use ww_api::prelude::*;

#[ww_api_root]
pub trait UartBridge {
    ww_impl!(uart[]: ww_uart::Uart);
}

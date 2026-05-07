#![no_std]

use wire_weaver::prelude::*;

#[ww_trait]
pub trait UartBridge {
    ww_impl!(uart[]: ww_uart::Uart);
}

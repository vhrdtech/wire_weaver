#![no_std]

use wire_weaver::prelude::*;

#[ww_api_root(wire = "w1.0")]
pub trait UartBridge {
    ww_impl!(uart[]: ww_uart::Uart);
}

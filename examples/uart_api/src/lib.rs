#![no_std]

use wire_weaver::prelude::*;

#[ww_trait]
pub trait UartBridge {
    ww_impl!(uart[]: "../../ww_stdlib/ww_uart/src/lib.rs" as ww_uart::Uart);
}

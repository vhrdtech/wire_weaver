# uart_api

> API for a USB/Ethernet/CAN to UART bridge.

This crate is used by several example firmwares in `examples_mcu/` folder.
Client code for Rust is generated from this crate in `../uart_api`.
Python wheel using the Rust client and this crate is in `../uart_api_py`.

This crate is `#![no_std]` by default, but if owned types that use alloc are desired, you can enable them via `std`
feature.

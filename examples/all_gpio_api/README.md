# blinky_api

> API for an examply blinky device.

This crate is used by several example firmwares in `examples_mcu/` folder.
Client code for Rust is generated from this crate in `../blinky`.
Python wheel using the Rust client and this crate is in `../blinky_py`.

This crate is `#![no_std]` by default, but if owned types that use alloc are desired, you can enable them via `std`
feature.

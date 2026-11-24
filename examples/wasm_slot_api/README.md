# wasm_slot_api

> API of the wasm slot example firmware: mount slots served by wasm interpreters, loaded from images in flash

`WasmSlot` mounts one slot per interpreter (`wasefire`, `wasmi`), loads and unloads the module images the firmware
carries, and reports each slot's counters, DWT cycles, stack and the heap. The firmware is
`examples_mcu/usb_stm32h725ig`'s `stm32h725ig-wasm-slot`, the client `examples/wasm_slot` (SRV-85,
`docs/api/mounts.md#wasm-modules`).

`#![no_std]`; the `std` feature adds owned types.

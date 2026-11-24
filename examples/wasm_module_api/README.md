# wasm_module_api

> API of the wasm module example: a calculator a firmware serves from a wasm module in a mount slot

`Calc` has `add`, `trap` (the module traps, the slot unloads it) and `spin` (an endless loop, stopped by wasmi's fuel).
`examples_mcu/wasm_module` compiles a server of it to wasm, the B135B firmware `stm32h725ig-wasm-slot` serves it in a
slot, and `examples/wasm_slot` calls it through a view of that slot (SRV-85, `docs/api/mounts.md#wasm-modules`).

`#![no_std]`; the `std` feature adds owned types.

# wasm_module_api_evolved

> The wasm module example API in a later version: what a reloaded module serves

`wasm_module_api`'s `Calc` with `mul` added at wire `w1.1`. The B135B firmware reloads a slot with this module, and
the client relearns that slot's API only (SRV-85, `docs/api/mounts.md#wasm-modules`).

`#![no_std]`; the `std` feature adds owned types.

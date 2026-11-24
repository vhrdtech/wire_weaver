# wasm_slot

> Client of the wasm slot example firmware on B135B: modules in mount slots, run by wasefire-interpreter and wasmi

Generated clients of `wasm_slot_api` (the firmware, over USB) and of `wasm_module_api` and `wasm_module_api_evolved`
(the modules, on a view of a slot). `tests/b135b.rs` checks the board: calls, a reload the client relearns, a trap
that unloads the module, wasmi's fuel, and prints the numbers of `docs/api/mounts.md#wasm-modules`. The tests are
ignored without the board: `just flash-wasm-slot`, then `just hw-wasm-slot`.

# ww_wasm changelog

## Unreleased

### 🚀 Features

- A WireWeaver server compiled to wasm serves a mount slot (SRV-85): the module ABI, `guest_module!` and `guest::Guest` for the guest, `wasefire::WasefireModule` (wasefire-interpreter) and `wasmi::WasmiModule` (wasmi, with `Limits` for fuel, stack and recursion) as `ww_device::mount::Module`s, `model_section`, `LoadError`.
- `wasmi::Limits::memory` and `table`: growth past them traps; a call's stack, code and event queue are allocated at load (SRV-87).
- `ww-wasm-prepare`: adds wasefire-interpreter's side table to a module on the host.

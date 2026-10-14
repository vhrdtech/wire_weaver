## Unreleased

### ⚠️ Breaking

- `Error` get `Unknown(u32)` from `derive_shrink_wrap` (SW-47): a variant from a newer version of this crate is read
  into it instead of failing. Exhaustive matches need an `Unknown(_)` arm. Same wire.
- `Error` gains `InUse`, `NoIoLevel`, `Refused` (STD-13).

### 🚀 Features

- `Pin::capabilities` returns `PinCapabilities`: modes, pulls, speeds, events, the pin's name.

### 🐛 Fixes

- `Bank` is `pub` (DOC-6: `#[ww_trait]` now leaves a real Rust trait, which must be visible for `ww_impl!` to use
  it from another crate).

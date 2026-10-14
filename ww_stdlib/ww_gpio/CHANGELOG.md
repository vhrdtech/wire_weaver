## Unreleased

### ⚠️ Breaking

- `Error` get `Unknown(u32)` from `derive_shrink_wrap` (SW-47): a variant from a newer version of this crate is read
  into it instead of failing. Exhaustive matches need an `Unknown(_)` arm. Same wire.

### 🐛 Fixes

- `Bank` trait is `pub`: it is used from other crates (`ww_impl!(port[]: ww_gpio::Bank)`), which needs the trait
  itself public now that `#[ww_trait]` emits one for rustdoc. No wire change.

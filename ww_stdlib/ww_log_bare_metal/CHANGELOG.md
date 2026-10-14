## Unreleased

### ⚠️ Breaking

- `Severity` get `Unknown(u32)` from `derive_shrink_wrap` (SW-47): a variant from a newer version of this crate is read
  into it instead of failing. Exhaustive matches need an `Unknown(_)` arm. Same wire.

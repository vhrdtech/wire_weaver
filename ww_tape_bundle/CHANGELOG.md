# ww_tape_bundle changelog

## Unreleased

### ⚠️ Breaking

- `key_in_bundle` returns a `ww_tape::TrackKey` (ww_tape READ-5).
- Declares `rust-version = "1.99"`: cargo of an older Rust refuses the crate.

### 🚀 Features

- First version: `name_path`, `key_in_bundle` and `NamePathError`, formerly `ww_tape`'s feature `bundle` (STD-125).
- Takes `ww_tape` by git from vhrdtech/ww_tape.

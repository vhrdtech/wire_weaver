# Changelog

## Unreleased

### ⚠️ Breaking

- `ww_std_cli` comes by git from vhrdtech/ww_stdlib; `ww_tools/Cargo.lock` takes it once ww_stdlib, logring and chunky_mcu_updater resolve together.

### 🚀 Features

- `ww logring` and `ww chunky` are built in, and `ww dfu flash` runs chunky's flash in process: no plugin needed (TOOL-39).
- New crate (TOOL-36): the `ww` binary, `wire_weaver_cli` and `ww_std_cli` linked; `cargo install --git .. ww`.

# Changelog

## Unreleased

### 📚 Documentation

- A README.

### ⚠️ Breaking

- Declares `rust-version = "1.99"`: cargo of an older Rust refuses the crate.

### 🚀 Features

- `--websocket URL` (`WW_WEBSOCKET`, `websocket` in ww.toml) connects to a device over WebSocket; `Selection::websocket`, `Selection::list_hint` (CLT-85).
- New crate. `DeviceArgs`, `Selection`, `Setting`, `Source`, `Untrusted`, `escape`, the device completers (`complete`) and the plugin protocol helpers (`plugin::complete_if_requested`, `selection_env`, `connect`, `take_exclusive`), moved out of `wire_weaver_cli` (TOOL-40). No `ww_hub` in its dependency tree.

# Changelog

## Unreleased

### 📚 Documentation

- A README.

### ⚠️ Breaking

- Declares `rust-version = "1.99"`: cargo of an older Rust refuses the crate.

### 🚀 Features

- New crate. `DeviceArgs`, `Selection`, `Setting`, `Source`, `Untrusted`, `escape`, the device completers (`complete`) and the plugin protocol helpers (`plugin::complete_if_requested`, `selection_env`, `connect`, `take_exclusive`), moved out of `wire_weaver_cli` (TOOL-40). No `ww_hub` in its dependency tree.

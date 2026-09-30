## Unreleased

### 🚀 Features

- New Python module `wire_weaver` (built with maturin): talk to any WireWeaver device without generated code, the
  API tree is built at runtime from the device introspection data. `connect(...)` with USB or RTT device filters
  returns a `Device` whose traits are attributes, arrays are indexed with `[]`, methods are called with positional or
  keyword arguments (`Result` returns raise `RemoteError` on `Err`), properties are `read()` / `write()`, streams are
  iterated or used as context managers, sinks `send()`. `repr()` shows the resource tree with signatures and docs,
  `dir()` lists resources for tab completion.
- `load_api(path)` builds the API from its crate source: pass it to `connect(api=...)` for a device with
  introspection disabled, or use it offline to browse the API and `encode` / `decode` values.
- `list_devices()` lists connected USB devices.
- RTT (probe-rs) is built in by default (`rtt` feature), the wheel supports every transport.
- `just py` (from the workspace root) opens a Python console with `ww` imported and the device
  connected as `dev`, see `scripts/console.py --help`.

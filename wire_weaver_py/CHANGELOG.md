## Unreleased

### 🚀 Features

- `Stream.open()` opens with `StreamOpen` (API-31); stream functions are named `plain` / `counted` / `reliable
  stream` in signatures.
- `Delta`, `DeltaOfDelta` and `XorFloat` (SW-50) are lists of `int` / `float` (any iterable accepted), a value out
  of range of the element type raises `ValueError`.
- Signatures of counted and reliable streams say so (`counted stream x: T`, API-30).
- `MaxLen` (SW-19) lists and strings are checked when encoding: a longer value raises `ValueError` (`.. at most N
  allowed ..`).
- Credit sinks (`#[credit] sink!(..)`, API-28, credit counted in bytes): `Sink.send()` waits until the device has
  room for the value (one bigger than the device's buffer raises `ww.WireWeaverError`), `Sink.flush()` waits until it accepted everything, `close()` flushes; `Sink.credit`, `Sink.progress` and
  `Sink.set_credit_config(query_interval, stall_timeout)`.
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

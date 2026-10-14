## Unreleased

### 🚀 Features

- Counted and reliable streams (CLT-23): a `PositionedStream` object with `open(*args, cursor=None, ack="receive")`
  (the stream function's arguments, its result returned and an `Err` raised as `ww.RemoteError`; resume from a saved `(session, pos)` cursor, `ack="explicit"` for `ack(pos)` after saving), `recv_event()`
  (dicts: items with `pos`, `units`, `t` in seconds, `run`; gaps with `at`, `units`, `cause`; `resumed`,
  `backfill_done`, `disconnected`, `closed`), `recv()` / iteration for values, `cursor`, `stats`, `delivery`,
  `close()`, a context manager. See `docs/python.md`.
- `Stream.open()` opens with `StreamOpen` (API-31); stream functions are named `plain` / `counted` / `reliable
  stream` in signatures.
- `Delta`, `DeltaOfDelta` and `XorFloat` (SW-50) are lists of `int` / `float` (any iterable accepted), a value out
  of range of the element type raises `ValueError`.
- Signatures of counted and reliable streams say so (`counted stream x: T`, API-30).
- Optional items (API-16): signatures end in `(optional)`, or `(left out by this device)` for one the device leaves
  out; calling such an item raises without sending a request.
- `RemoteError.common` (STD-16): the `ww_error::CommonError` in a method's error (`{"InUse": {"by": "USART2"}}`),
  `None` for an error of the API's own. The message is the error variant's doc line with its fields filled in
  (`read() returned an error: In use by USART2`) instead of the value's repr, where the variant has one.
- `Method.decode_return(data, raise_err=True)` returns `T` or raises `RemoteError` as a call would.
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

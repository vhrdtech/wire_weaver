## Unreleased

### ⚠️ Breaking

- **Breaking:** credit sinks have no 4 GiB session limit any more (API-42), positions wrap on the wire at 2^28; a
  session over 256 MiB needs a device with the same change. `Sink.progress` is unchanged for Python.

### 🚀 Features

- `ww.Quantity` (CLT-31): a `ww_si` quantity (ww_si 0.2 and later) decodes to an object with `value`, `prefix`,
  `exponent`, `storage`, `symbol`, `unit`, `dim`, `docs`, `to_si()`, `float()` and `str()` (`3300 mV`); writes take a
  `Quantity` of the same unit or a number in the unit itself (an `int` as `I32` / `I64`, a `float` as `F64`), the
  `{"prefix": .., "value": {..}}` form still works. Decoded quantities used to be that dict.
- `Series<T>` (SW-60, CLT-29) values are a `list` of items; `decode_columns(data)` on a method, property or stream
  of a `Series` gives a `dict` of field path -> numpy array over all items, masked arrays for leaves some items
  don't have (another variant, `None`). numpy is imported then, not a dependency. See `docs/python.md#values`.
- Self-referential types (API-44): `load_api` takes APIs with trees and expressions (`RefBox`, a list of the type
  itself), values encode and decode as nested dicts and lists. An object nested deeper than a dynamic decoder reads
  (`ww_self::MAX_DECODE_DEPTH`), or one that contains itself, is a `ValueError` instead of a stack overflow.

- Generic types (API-43): `load_api` instantiates them, `Timed<Ampere>`, `Window<4>` encode and decode like any
  struct; signatures read `rw outer: Outer<3>`.

- `Node.observe(cfg)` (TOOL-11): observes a trait level's `+observe` properties without generated code, from
  introspection alone. Iterate the result (or call `recv()` / `try_recv()`) for a dict per update (`{"values": {name:
  value, ...}, "t": seconds or None, "missed": int, "bytes": int}`); `cfg` is a dict `{property_name: rate_ms}`
  (`rate_ms` a number for periodic, `None` for on-change), or `None` to observe every observable property on-change.
  `budget`, `dropped`, `configure(cfg)`. `Node.observable_properties()` lists a level's `+observe` properties by
  name and type. See `docs/python.md#observing`.
- Reading a `const` item asks the device once per connection, later reads return that value (API-36).
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
- A method's `#[errors(..)]` (API-32) is in its docs (`help()`, `.doc`): `Errors:` with each entry and its variant's
  doc line. A device returning an error outside the list logs a warning.
- The `#[errors(..)]` of stream functions and properties (API-32) is in their docs too; an error outside the list from
  opening a counted or reliable stream, or reading or writing a property, logs a warning.
- Optional items (API-16): signatures end in `(optional)`, or `(left out by this device)` for one the device leaves
  out; calling such an item raises without sending a request.
- `RemoteError.common` (STD-16): the `ww_error::CommonError` in a method's error (`{"InUse": {"by": "USART2"}}`),
  `None` for an error of the API's own. The message is the error variant's doc line with its fields filled in
  (`read() returned an error: In use by USART2`) instead of the value's repr, where the variant has one.
- `Method.decode_return(data, raise_err=True)` returns `T` or raises `RemoteError` as a call would.
- `UVlq32` / `UVlq64` numbers (STD-23) are Python `int`s, both ways.
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

### 🐛 Fixes

- `connect()` decodes values of a type the device left out under a signature its snapshot doesn't have (a build from
  another revision of an unpublished crate, e.g. `ww_si::Volt`) without `load_api`, by taking the definition from the
  definition cache `~/.wire_weaver/definitions/` (EVO-9).


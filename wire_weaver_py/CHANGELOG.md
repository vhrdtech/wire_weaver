## Unreleased

### ⚠️ Breaking

- `Quantity.prefix` is a `str` of any scale (`"Ki"` for a binary one) and `Quantity.exponent` is `None` for a scale that is no power of ten (CLT-54).
- `Quantity.dim` has `rad` and `bit` and counted bases by name (`{"Sample": 1, "s": -1}`), STD-60: a torque per angular velocity is `{"kg": 1, "m": 2, "s": -1, "rad": -2}`. A square root (V/√Hz) has halves: `{"kg": 1, "m": 2, "s": -2.5, "A": -1}`.
- Credit sinks on the stream protocol (API-34): a device with the same WireWeaver is needed; `Sink.progress` counts units (one per value, the bytes of a byte sink's payload), no longer bytes.

- `Delta`, `DeltaOfDelta` and `XorFloat` values are gone with the types (shrink_wrap): their replacement `Series<T>` of
  numbers takes and gives the same Python `list` (any iterable accepted for `encode`).
- **Breaking:** the `rtt` feature is no longer in `default-features` (P2401): building the crate directly with
  plain `cargo build`/`cargo check` no longer pulls in `probe-rs` (and its `hidapi` dependency). The `pip`-installed
  wheel is unaffected — `pyproject.toml`'s `[tool.maturin] features = ["rtt"]` enables it for the actual build.

- **Breaking:** credit sinks have no 4 GiB session limit any more (API-42), positions wrap on the wire at 2^28; a
  session over 256 MiB needs a device with the same change. `Sink.progress` is unchanged for Python.

### 🚀 Features

- `exact_units::Fixed` values decode as `ww.Quantity` (CLT-54): `value` as stored, `prefix` (`"Micro"`, `"Ki"`), `storage` as the type names it (`"u64"`), `symbol`, `unit`, `to_si()`, new `fixed` (true for a `Fixed`) and `bounds` (`(1, 5000)` or `None`); `str()` is exact (`1730000000000000 µs`, `4 KiB`). `encode` takes a `Quantity`, converted exactly into the `Fixed`'s scale and storage or `ValueError` naming the type, or a number as stored. `Index`, `IndexFrom1` and `Seq` take their number alone too, checked against bounds and an `IndexFrom1`'s 0.
- `ww.requantize(value, factor, to, rounding, overflow="checked")` and `ww.Requantizer` for a series of values (CLT-54, STD-63): a stored number converted to another scale and storage with a named rounding (`"truncate"`, `"floor"`, `"ceil"`, `"nearest_even"`, `"nearest_away"`) and overflow policy (`"checked"` gives `None`, `"saturate"`, `"wrap"`), `exact_units::Requantizer` underneath, so the same number as the typed `Fixed::requantize`, bit for bit. The factor is a `Magnitude` as the model has it: its wire bytes or the `dict` `decode` gives for one. The requantize cases of the conformance corpus run in `tests/test_conformance.py`.
- A `Quantity` in a unit of the API's own crate declared with `unit!` (INT-13) has its symbol, `dim` and `to_si()` from the definition the API carries (`1500 rpm`, `{"rad": 1, "s": -1}`) and goes into an `AnyQuantity`; before it was known by name and docs only.
- `Quantity.convert(unit)` (STD-72): the quantity in another unit, given by its type name (`"Times<Newton, Second>"`) or as another quantity: exactly or `ValueError`, never the number read in another unit (the Mars Climate Orbiter case).
- The ww hub from Python and notebooks (TOOL-18): `python -m wire_weaver.hub` serves the hub (found by `connect()`'s hub routing the same way as a Rust `ww` executable, CLT-39: `sys.executable -m wire_weaver.hub` is registered at import), `connect()` takes `hub_device` (a device key through the hub, `WW_HUB`/`WW_HUB_OPENS` route the USB/RTT filters the same as the Rust client), and `wire_weaver.hub()` connects to the running hub's own API (`ww_hub_api`) through the dynamic client, starting one if none runs: `hub().registry.devices()`, `.clients()`, `for e in hub().registry.events: ...`. The wheel now carries `wire_weaver_client` with its `hub` feature and `ww_hub`.
- The package is now a mixed Python/Rust project (`maturin`'s `python-source`): the compiled extension lives at `wire_weaver._wire_weaver`, re-exported by `wire_weaver/__init__.py` plus the new `wire_weaver/hub.py`; `import wire_weaver as ww` is unchanged.
- `connect()` tells the device it is a notebook (`ClientKind::Notebook` in the link setup client meta, CLT-41).

- `PositionedStream.stats` has `lost_codec`, `codec_bytes_in` and `codec_bytes_out`: counted streams come compressed from devices that compress them (API-55), decoded on the way.

- Credit sinks resume (API-34): `Sink.cursor` (`(session, pos)`, where the device is), `Sink.resume(cursor)` continues a session after a restart of the program and returns the position where the device is, `Sink.open(session=n)` opens a session the program names; `ww.SessionLost` (a `WireWeaverError`) when the device lost the session.

- `Sink.send(value, units=n)` (API-34): a value that takes `n` positions of a credit sink.

- `encode` / `encode_args` of a `Series<T>` (CLT-29): a list of items is written with the bytes the generated
  code gives (`ww_self`'s dynamic `Series` writer); it was an error before.
- `PositionedStream.recv_batch()` / `Stream.recv_batch()` (API-41): the rest of the next batch, or the next one
  whole, of a batched stream, as a `dict` instead of one item at a time: `pos` (`PositionedStream` only), `t`
  (seconds of device time, a numpy array, `None` for an untimed stream) and the item type's own fields, each a
  numpy array over the batch's items, same keys and shape as `decode_columns()`. Built from the already-decoded
  items (`ww_self::SeriesColumn::from_items`), not a re-read of the batch's bytes. numpy is imported then, not a
  dependency.

- Batched observe (API-39): `observe({"name": {"samples": max_age_ms, "every": ms}})` observes every sample of a
  `+batched` property; `recv()` dicts get `"samples": {name: [(t, value), ...]}` (device times unwrapped, from the
  update's shared `sample_times`, API-49),
  `Observer.recv_batch()` gives them as numpy columns per property (`"batches": {name: {"t": .., ...}}`, keyed as a
  batched stream's `recv_batch()`).

- `ww_si::AnyQuantity` values (STD-37) decode to `ww.Quantity`, the same as the typed quantity of their unit, and take
  any `ww.Quantity` whose unit has a dimension on write; `dim` and `to_si()` work for a unit a device sent with its
  dimension and scale.
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

- Decoding an enum value with a variant from a newer version (an `Unsized` enum, SW-16) gave `ValueError: unknown
  variant <unknown N>`; it now gives the string `"<unknown N>"`, as the dynamic decoder names it. Found by the
  conformance matrix (TEST-23).
- `connect()` decodes values of a type the device left out under a signature its snapshot doesn't have (a build from
  another revision of an unpublished crate, e.g. `ww_si::Volt`) without `load_api`, by taking the definition from the
  definition cache `~/.wire_weaver/definitions/` (EVO-9).


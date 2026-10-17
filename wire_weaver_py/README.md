# wire_weaver (Python)

> Talk to any WireWeaver device from Python, without generated code.

Full documentation: `docs/python.md` (Dev tools → Python on the docs site).

The API tree (traits, methods, properties, streams) is built at runtime from the introspection data a device sends
(cached in `~/.wire_weaver/`), or from the API crate source. Values are converted to and from plain Python objects,
and are serialized exactly as the generated Rust clients do it (`wire_weaver_client::DynResource` underneath).

```python
import wire_weaver as ww

print(ww.list_devices())
dev = ww.connect(vid_pid=(0xc0de, 0xcafe), timeout=1.0)
dev                                    # resource tree with signatures and docs

dev.led_on()
dev.add(1, b=2)                        # positional or keyword arguments
dev.channel.valid_indices()            # [0, 1, 2]
dev.channel[1].gain.write(0.5)
dev.channel[1].gain.read()

with dev.samples as samples:           # stream from the device
    for s in samples:
        ...
with dev.commands as sink:             # stream to the device
    sink.send({"Move": {"x": 1, "y": 2}})

dev.disconnect()
```

A sink marked `#[credit]` in the API (`sink.credit` is `True`) has flow control: `send()` waits until the device has
room for all bytes of the value (a value bigger than the device's buffer raises `ww.WireWeaverError`), `flush()`
waits until it accepted everything (lost values are sent again), `close()` flushes, `sink.progress` is
`(accepted, sent)` bytes, `sink.set_credit_config(query_interval=1.0, stall_timeout=30.0)` tunes it
to a slow link. See `docs/api/streams.md`.

A method returning `Result<T, E>` returns `T`, or raises `ww.RemoteError` with `E` in its `value` attribute and the
errors every API shares (`ww_error::CommonError`, e.g. `{"InUse": {"by": "USART2"}}`) in `common` (`None` for an error
of the API's own); the message is the error's doc line (`In use by USART2`). A request
without a reply raises `TimeoutError`, other errors raise `ww.WireWeaverError`. Use `dev["name"]` for a resource named
like one of the `Device` methods (`api`, `info`, `disconnect`).

A device with introspection disabled needs its API from the crate source:

```python
api = ww.load_api("path/to/my_api", "MyApi")   # trait name is needed if there are several
dev = ww.connect(api=api)
```

`load_api` alone also works offline: browse the API and serialize values without a device, e.g.
`api.add.encode_args(1, 2)`, `api.speed.encode(1500)`, `api.speed.decode(b"...")`.

## Values

| WireWeaver                      | Python                                                               |
|---------------------------------|----------------------------------------------------------------------|
| `bool`                          | `bool`                                                               |
| integers, `UNib32`, `u4`, `nib` | `int` (range is checked)                                             |
| `f32`, `f64`                    | `float` (`int` accepted)                                             |
| `String`, `&str`                | `str`                                                                |
| `Vec<u8>`, `[u8; N]`            | `bytes` (`bytearray` or a list of ints accepted)                     |
| `Vec<T>`, `[T; N]`              | `list` (any iterable accepted)                                       |
| tuple                           | `tuple`                                                              |
| struct with named fields        | `dict` (dataclass, namedtuple or an object with attributes accepted) |
| tuple struct                    | `tuple`, a single field one is its value                             |
| enum unit variant               | `"Variant"`                                                          |
| enum variant with fields        | `{"Variant": fields}`, fields as for a struct                        |
| `Option<T>`                     | `None` or `T`, a missing `Option` field is `None`                    |
| `Result<T, E>`                  | `{"Ok": T}` or `{"Err": E}`                                          |
| `Range`, `RangeInclusive`       | `(start, end)` (`range` with step 1 accepted)                        |
| `ww_si` quantity (`Volt`, ..)   | `ww.Quantity` (a `Quantity` of the same unit, an `int` or `float` in the unit itself accepted) |
| `ww_si::AnyQuantity`            | `ww.Quantity` (any `Quantity`, its unit goes along)                  |

A `ww.Quantity` (ww_si 0.2 and later) keeps what the device sent and knows its unit:

```python
q = dev.supply.read()        # Quantity(3300 mV U16)
str(q), float(q)             # "3300 mV", 3.3 (in the unit itself)
q.value, q.prefix, q.exponent, q.storage   # 3300, "Milli", -3, "U16"
q.symbol, q.unit, q.dim      # "V", "Volt", {"kg": 1, "m": 2, "s": -3, "A": -1}
q.to_si()                    # 3.3, through the unit's scale: 90 ° is 1.5708 (rad), 20 °C is 293.15 (K)
dev.supply.write(1.5)        # 1.5 V
```

Compound units are written out (`N·m/(rad/s)` for `Quantity<Per<Torque, AngularVelocity>>`); a unit marker `ww_si`
doesn't know (an API's own `struct Rpm;`) has its name as `symbol`, its docs in `docs`, `dim` is `None` and `to_si()`
raises `ValueError`.

An `AnyQuantity` (a value whose unit is only known at run time, STD-37) is a `ww.Quantity` too, the same as the typed
quantity of its unit (`==` holds). A unit the device sent that `ww_si` doesn't have keeps its symbol, `dim` and
scale, so `to_si()` works; writing takes any `ww.Quantity` whose unit has a dimension (not a marker known by name only).

## Interactive console

From the workspace root, `just py` builds the module (when Rust sources changed) and opens a Python prompt with `ww`
imported and the device connected as `dev`, if exactly one is found or a filter is given:

```sh
just py                                  # the only connected USB device
just py --serial 0123 --timeout 5        # pick one
just py --api examples/blinky_api        # API from source: device without introspection, or offline browsing
just py --rtt STM32G0B1RETx --elf fw.elf      # over RTT (debug probe)
just py --help
```

## Building

```sh
just test-py                       # build into wire_weaver_py/.venv and run the tests
uvx maturin build --release        # wheel in target/wheels/
uvx maturin develop                # install into the active venv
```

RTT support (probe-rs) is the default `rtt` feature, pure Rust, so the wheel needs no system libraries. Leave it out
with `--no-default-features` for a smaller wheel.

## Not supported yet

- `f16`, `UN`/`IN` and LEB128 numbers.
- Typed (non-integer) array indices, property write errors (not decoded, same as generated clients), property
  observation.
- asyncio: calls block the calling thread (with the GIL released).

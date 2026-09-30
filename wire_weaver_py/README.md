# wire_weaver (Python)

> Talk to any WireWeaver device from Python, without generated code.

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

A method returning `Result<T, E>` returns `T`, or raises `ww.RemoteError` with `E` in its `value` attribute. A request
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

## Interactive console

From the workspace root, `just py` builds the module (when Rust sources changed) and opens a Python prompt with `ww`
imported and the device connected as `dev`, if exactly one is found or a filter is given:

```sh
just py                                  # the only connected USB device
just py --serial 0123 --timeout 5        # pick one
just py --api examples/blinky_api        # API from source: device without introspection, or offline browsing
just py-rtt --rtt STM32G0B1RETx --elf fw.elf   # over RTT (module built with the rtt feature)
just py --help
```

## Building

```sh
just test-py                       # build into wire_weaver_py/.venv and run the tests
uvx maturin build --release        # wheel in target/wheels/
uvx maturin develop                # install into the active venv
```

RTT support needs the `rtt` feature: `uvx maturin build --release --features rtt`.

## Not supported yet

- `f16`, `UN`/`IN` and LEB128 numbers.
- Typed (non-integer) array indices, property write errors (not decoded, same as generated clients), property
  observation.
- asyncio: calls block the calling thread (with the GIL released).

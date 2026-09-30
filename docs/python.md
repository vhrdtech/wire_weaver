# Python

The `wire_weaver` Python module talks to any WireWeaver device without generated code. The API tree (traits, methods,
properties, streams) is built at runtime from the introspection data the device sends, or from the API crate source.
Values are converted to and from plain Python objects and serialized exactly as the generated Rust clients do it,
the module is a thin wrapper around the dynamic client in `wire_weaver_client`.

It is meant for debugging, quick tests and scripts: connect, look at what the device offers, call it. For a
typed Python package wrapping one specific API, see the `blinky_py` example instead ([Examples](examples/examples.md)),
it wraps the generated Rust client.

Both transports are built in: [USB](transport/usb.md) and [RTT](transport/rtt.md) over a debug probe (probe-rs).
Everything is pure Rust, the wheel needs no system libraries.

## Installation

The module is built with [maturin](https://www.maturin.rs), from a checkout of the repository (a Rust toolchain is
needed):

```sh
cd wire_weaver_py
uvx maturin build --release        # wheel in target/wheels/, pip install it anywhere
uvx maturin develop --release      # or install into the active virtual environment
```

`--no-default-features` leaves RTT out for a smaller wheel.

!!! note "Linux permissions"
    Listing devices works without any setup, but opening one needs read/write access to `/dev/bus/usb/...`,
    usually granted to your user with a udev rule. See [USB](transport/usb.md).

## Interactive console

From the repository root, `just py` builds the module into `wire_weaver_py/.venv` (only when Rust sources changed)
and opens a Python prompt with `ww` imported and the device connected as `dev`, if exactly one is found or a filter
is given:

```sh
just py                                        # the only connected USB device
just py --serial 0123 --timeout 5              # pick one: --serial, --label, --vid-pid c0de:cafe
just py --rtt STM32G0B1RETx --elf fw.elf       # over RTT, the ELF makes attaching fast
just py --api examples/blinky_api              # API from source: device without introspection, or offline
just py --help
```

Relative paths are resolved from the directory `just` was run in.

## Connecting

```python
import wire_weaver as ww

print(ww.list_devices())                       # connected USB devices, one line each
dev = ww.connect(vid_pid=(0xc0de, 0xcafe), timeout=1.0)
dev.info                                       # API, link and protocol versions reported by the device
dev                                            # resource tree with signatures and docs
dev.disconnect()                               # or `with ww.connect(...) as dev:`
```

`connect()` takes keyword arguments only. A USB device is used unless `rtt` is given; all given filters must match:

| Argument                                      | Selects                                                                  |
|-----------------------------------------------|--------------------------------------------------------------------------|
| `vid_pid=(vid, pid)`                          | USB vendor and product ID                                                |
| `serial`, `serial_contains`                   | USB serial number                                                        |
| `user_label`                                  | user label reported by the device                                        |
| `product_contains`, `manufacturer_contains`   | USB product / manufacturer string                                        |
| `implements_api=("my_api", ">=0.2")`          | API crate name and a SemVer requirement                                  |
| `rtt="STM32G0B1RETx"`                         | RTT over a debug probe, probe-rs chip name                               |
| `rtt_elf="fw.elf"`, `rtt_speed_hz`            | firmware ELF to take the RTT control block address from, probe speed     |
| `timeout`                                     | default request timeout in seconds                                       |
| `api`                                         | API from `load_api()`, for a device with introspection disabled          |

The introspection data is cached in `~/.wire_weaver/`, so reconnecting to a device with the same API is quick.
Without `rtt_elf`, RTT attaching scans the target RAM for the control block, see [RTT](transport/rtt.md#host-side).

## Using the API

Resources are attributes, their `repr()` shows the tree with signatures and the first doc line, and `dir()` lists
them for tab completion. For the `Dynamic` trait from `tests/dynamic_api`:

```python
>>> dev
Dynamic
  no_args()
  add(a: u32, b: i16) -> i64
  echo(value: Everything) -> Everything
  check(x: Option<u8>) -> Result<u8, CheckError>
  rw speed: u16
  rw everything: Everything
  rw flagged: Flagged
  channel[]: Channel
>>> dev.channel[0]
channel[0]: Channel
  rw gain: f32
  id() -> u32
```

```python
dev.no_args()
dev.add(40, b=-2)                      # positional or keyword arguments
dev.speed.write(1500)                  # properties
dev.speed.read(timeout=5)              # timeout overrides the default for one request
dev.channel.valid_indices()            # arrays: indices the device accepts, e.g. [0, 1, 2]
dev.channel[1].gain.write(0.5)

with dev.samples as samples:           # stream from the device: open(), iterate or recv(), close()
    for s in samples:
        ...
dev.samples.recv(timeout=1)            # after open(): next item, TimeoutError if none
dev.samples.try_recv()                 # next item if one is already received, None otherwise

with dev.commands as sink:             # stream to the device (sink): open(), send(), close()
    sink.send({"Move": {"x": 1, "y": 2}})
```

Every resource also has `doc` (its doc comment), `path` (e.g. `channel[2].gain`) and, except traits, `signature`.
Use `dev["name"]` for a resource named like one of the `Device` attributes (`api`, `info`, `disconnect`).

Blocking calls release the GIL, and waiting on a stream can be interrupted with Ctrl+C.

### Errors

- A method returning `Result<T, E>` returns `T`, or raises `ww.RemoteError` with `E` in its `value` attribute.
- A request without a reply raises `TimeoutError`.
- Connection, protocol and device errors raise `ww.WireWeaverError` (`RemoteError` is a subclass of it).
- Values that don't fit the API types raise `TypeError` or `ValueError` before anything is sent, e.g.
  `add() argument 'a': -1 is out of range for u32`.

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

## API from source, offline use

A device with introspection disabled needs its API from the crate source:

```python
api = ww.load_api("path/to/my_api", "MyApi")   # trait name is needed if the crate has several
dev = ww.connect(api=api)
```

`load_api()` alone works without a device: browse the API the same way, and serialize values to see what goes over
the wire, or decode captured bytes:

```python
>>> api = ww.load_api("tests/dynamic_api", "Dynamic")
>>> api.add.encode_args(40, b=-2).hex()
'28000000feff'
>>> api.speed.encode(1500)
b'\xdc\x05'
>>> api.speed.decode(b"\xdc\x05")
1500
>>> api.check.decode_return(b"\x00\x00\x01")
{'Err': 'TooBig'}
```

Methods have `encode_args()` / `decode_return()`, properties `encode()` / `decode()`, streams `decode()` and sinks
`encode()`. Calling a
resource of an offline API raises `WireWeaverError`.

## Not supported yet

- `f16`, `UN`/`IN` and LEB128 numbers.
- Typed (non-integer) array indices, property write errors (not decoded, same as generated clients), property
  observation.
- asyncio: calls block the calling thread (with the GIL released).

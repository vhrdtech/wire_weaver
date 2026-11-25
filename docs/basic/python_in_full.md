---
level: basic
---

# Python in full

The earlier chapters used Python for a line or two at a time. This chapter goes through the rest of the `wire_weaver`
Python module on the thermometer: how to look around a device's API, how values and units look in Python, how to
read a stream and feed a sink, and where each kind of error comes from. It ends with the interactive console and
the device's log files.

The module needs no code generated for your API. It reads the API from the device when it connects, through
[[introspection]], so the same module talks to any WireWeaver device. Every script of this chapter is in
`examples/thermometer_py`. Start the simulated thermometer in a terminal of its own, with a room that keeps its
temperature, so that your output matches the book's:

```sh
cargo run -p thermometer --example thermometer_sim -- --still
```

Then run each script from `examples/thermometer_py` with `uv run python SCRIPT.py`. Inside this repository, uv builds
the module from the checkout; elsewhere, install the `wire_weaver` wheel from
[vhrdtech/ww_tools](https://github.com/vhrdtech/ww_tools).

## Looking around

A connected device prints itself as the tree of its API, with each item's signature and the first line of its doc
comment. [`examples/thermometer_py/explore.py`] connects to the simulated thermometer and prints it:

```python
--8<-- "examples/thermometer_py/explore.py:tree"
```

Every item is an attribute of the device, and every attribute tells about itself. `signature` is the item's line of
the tree, `doc` its whole doc comment. An array such as `channel[]` says which indices the device accepts, and an item
inside it knows its `path`:

```python
--8<-- "examples/thermometer_py/explore.py:items"
```

The script prints:

```text
--8<-- "docs/generated/thermometer/explore_py.txt"
```

`dir(dev)` lists the same items, so an interactive prompt completes them with Tab.

## Values

Python holds each value of the API in the closest Python type. A struct is a `dict` of its fields, an enum variant
without fields is its name as a string, a list is a `list`. A number with a unit is a `ww.Quantity`. The table lists
the common types:

| In the API | In Python |
|---|---|
| `bool`, integers, `f32`, `f64` | `bool`, `int`, `float` (an integer's range is checked) |
| `String`, `&str` | `str` |
| `Vec<u8>`, `[u8; N]` | `bytes` |
| `Vec<T>`, `[T; N]` | `list` |
| struct with named fields | `dict` of its fields |
| enum variant without fields | `"Variant"` |
| enum variant with fields | `{"Variant": fields}` |
| `Option<T>` | `None` or the value |
| `Fixed`, `Point`, `ww_si` quantities | `ww.Quantity` |
| `Index`, `IndexFrom1`, `Seq` | `dict` of the number, as a struct |

[`examples/thermometer_py/values.py`] takes one sample with `read_once`. The `Sample` struct comes back as a `dict`.
Its sequence number and its input are each a `dict` of one number:

```python
--8<-- "examples/thermometer_py/values.py:struct"
```

The `Python` column of the table is also what you write. A method's arguments are Python values, positional or by
name, and a property's `write` takes one. The module checks each against the API's type before it sends anything.

## Numbers with units

The sample's temperature is a `Point<Celsius, Fixed<Kelvin, Centi, i16>>` in the API, and a `ww.Quantity` in Python.
A quantity knows its unit, its step and the number it stores:

```python
--8<-- "examples/thermometer_py/values.py:quantity"
```

`str()` shows it as the device means it, in degrees Celsius. `float()` is the value in that unit. `value` is the
stored number, 2150 steps of 0.01 K, and `prefix` names the step. `origin` names the zero of a temperature scale, and
`to_si()` converts to kelvin.

To write a number with a unit, build a quantity from text. The module converts it exactly into the type's step and
storage, or refuses it. A plain number is the stored number, as a Rust client writes it:

```python
--8<-- "examples/thermometer_py/values.py:write"
```

`set_rate` takes `Fixed<Hertz, One, u16, Range<1, 1000>>`. 5 Hz and 0.02 kHz both convert exactly, and 10 is ten
steps of 1 Hz. The three refused values show the three checks: the dimension, the step and the [[bounds]].

An enum property reads and writes its variant by name:

```python
--8<-- "examples/thermometer_py/values.py:enum"
```

The whole script prints:

```text
--8<-- "docs/generated/thermometer/values_py.txt"
```

## Streams and sinks

A [[stream]] carries items from the device, and a [[sink]] carries items to it. In Python both open with `with`, which
also closes them at the end of the block. [`examples/thermometer_py/streams.py`] first disables inputs 2 to 4, so
that every sample comes from input 1. It then opens the `samples` stream, starts the sampler at 5 Hz and receives four
samples:

```python
--8<-- "examples/thermometer_py/streams.py:stream"
```

`recv(timeout=2)` waits up to two seconds for the next item and raises `TimeoutError` when none comes. A `for` loop
over the open stream receives its items one by one, too.

The `commands` sink takes the thermometer's `Command` enum. In `Triggered` mode, the device takes one sample for each
`Trigger`. A `Mark` sets the next sequence number first:

```python
--8<-- "examples/thermometer_py/streams.py:sink"
```

The sink delivers its items in order, so the triggered sample carries the number the `Mark` set. The script prints:

```text
--8<-- "docs/generated/thermometer/streams_py.txt"
```

## Errors

An error comes from one of two places: the module refuses a value before it sends anything, or the device refuses a
request. The module raises `TypeError` for a value of the wrong kind and `ValueError` for a value outside its type, as
[`examples/thermometer_py/errors.py`] shows:

```python
--8<-- "examples/thermometer_py/errors.py:local"
```

A request the device refuses raises `ww.WireWeaverError`. Here the thermometer has no input with index 7:

```python
--8<-- "examples/thermometer_py/errors.py:device"
```

The script prints:

```text
--8<-- "docs/generated/thermometer/errors_py.txt"
```

A method whose return type is `Result<T, E>` returns `T` when it succeeds. When the device returns an `Err`, the call
raises `ww.RemoteError`, a kind of `WireWeaverError`, with the error in its `value` attribute. The bench supply's test
bench in [`examples/bench_supply_py/tests/test_bench_supply.py`] reads the current that the load would have drawn
from such an error:

```python
--8<-- "examples/bench_supply_py/tests/test_bench_supply.py:remote_error"
```

A request without a reply raises `TimeoutError`. The `timeout` argument of `ww.connect()` sets the default in
seconds, and a property's `read(timeout=..)` overrides it for one request.

## The console

For trying things out, the ww_tools repository has an interactive prompt with the module imported as `ww` and a USB
device connected as `dev`. From a checkout of [vhrdtech/ww_tools](https://github.com/vhrdtech/ww_tools), run:

```sh
just py --serial 0123
```

`just py --help` lists the other ways to pick a device: by its label, its USB vendor and product IDs, its debug probe,
or the API's source for a device without introspection. With `--no-connect` the prompt only imports the module, and
`dev = ww.connect(websocket_url="ws://127.0.0.1:8080/ww")` reaches the simulated thermometer from there.

## Log files

A device that keeps a ring log writes it to a card, a file or flash. The shipped wheel holds the Python API of
[logring](https://github.com/vhrdtech/logring) as `wire_weaver.logring`: it opens such a log and reads its tracks, for
example into pandas. The [logring](../logring.md) page links to logring's own book, which shows the API.

## Where to go next

- [Python: streams, observing, offline](../python_advanced.md) covers counted and reliable streams, observing a group
  of properties, and an API read from its source without a device.
- [Python](../python.md) lists every argument of `ww.connect()` and every value conversion.

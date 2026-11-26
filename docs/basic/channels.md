---
level: basic
---

# Channels

The thermometer has up to four temperature inputs, and each has the same two things: a switch that says whether the
sampler reads it, and the last sample taken on it. Writing `enabled_1` to `enabled_4` into the root trait would work
for one board, but a board with two inputs or eight would need another API. WireWeaver instead groups the items of
one input into a trait of their own, and serves that trait as an array whose length the device decides. This
chapter shows the thermometer's channels and its status LED, both such groups, from the API to Rust and Python.

Start the simulated device in a terminal of its own, and leave it running while you read:

```sh
cargo run -p thermometer --example thermometer_sim
```

## A trait for one input

A [[sub-trait]] is an API trait that another trait puts inside itself. The thermometer's `Channel` holds what one
input has:

```rust
--8<-- "examples/thermometer_api/src/lib.rs:channel"
```

It is declared like the root trait, with `#[ww_trait]` instead of `#[ww_api_root]`. The root trait, shown in
[Properties](properties.md#declaring-a-property), puts it in with `ww_impl!(channel[]: Channel)`. The `[]` after the
name makes `channel` an [[array]]: one `Channel` per input. The status LED is a sub-trait too, `Indication`, put in
once with `ww_impl!(indication: Indication)`, without `[]`.

The API doesn't say how long the array is. The device answers that at run time, so the same API serves a board with
four inputs and one with two.

## The API as a tree

Sub-traits make the API a tree. The figure draws the thermometer's tree from the [[introspection]] data the simulated
device sends. Next to each item is its index chain: the numbers a request carries to reach the item. An item's number
is its place in its trait, counted from 0. An element of an array adds its index.

--8<-- "docs/generated/thermometer/channel_tree.svg"

A request to `enabled` of the third input carries `[6, 2, 0]`: item 6 of the root, `channel`, then index 2, then
item 0 of `Channel`, `enabled`. Array indices on the wire count from 0. The thermometer's `channel[2]` is the input
printed 3 on the board, and its samples say `input 3`.

`ww introspect` shows the same tree as text, as in [Numbers that know their unit](units.md#from-the-command-line).

## Serving the channels

The device says which indices exist with one [[handler]] per array, `valid_indices_root_channel`. The thermometer has
as many channels as its board has inputs:

```rust
--8<-- "examples/thermometer_device/src/lib.rs:valid"
```

A request for another index never reaches a handler: the generated code answers it with the error `BadIndex`. `input`
turns a valid index into the board's input number, which counts from 1.

The handlers of a sub-trait's items are named after the path to them: `set_channel_enabled`, `get_channel_enabled`
and `channel_last`. An item inside an array gets one more argument, `index_chain`, with the indices on the way to
it:

```rust
--8<-- "examples/thermometer_device/src/lib.rs:enabled"
```

The handlers still check the index, because the type of `index_chain` doesn't say it was checked. A request with an
index outside the board's inputs gets the error `NO_SUCH_INPUT`, a `CommonError::Refused` with a reason. The status
LED's handlers, `set_indication_mode` and `get_indication_mode`, have no `index_chain`, because `indication` is not an
array.

## From Rust

The generated [[client]] reaches a sub-trait through a function named after it. `channel(index)` returns the client
of one channel, with `read_enabled()`, `write_enabled(value)` and `last()`. `channel_valid_indices()` asks the device
which indices exist. The program [`examples/thermometer/examples/channels.rs`] lists every channel:

```rust
--8<-- "examples/thermometer/examples/channels.rs:list"
```

It then turns one channel off, and asks for a channel the board doesn't have:

```rust
--8<-- "examples/thermometer/examples/channels.rs:one"
```

A sub-trait that is not an array is reached the same way, without an index:

```rust
--8<-- "examples/thermometer/examples/channels.rs:indication"
```

Run it in a second terminal, while the simulated device runs in the first:

```sh
cargo run -p thermometer --example thermometer_channels
```

It prints:

```text
--8<-- "docs/generated/thermometer/channels.txt"
```

The four inputs read 0.25 °C apart, as the simulated board places them. `channel[4]` fails with `BadIndex`, and its
handler never runs. A disabled channel still answers `last()`, but the sampler skips its input: the continuous samples
in [Streams and sinks](streams.md) come from the enabled inputs only.

## From Python

The Python module builds the same tree at run time. An array is indexed with `[]`, and `valid_indices()` lists its
indices:

```python
--8<-- "examples/thermometer_py/channels.py:channels"
```

A sub-trait without `[]` is an attribute like any other item:

```python
--8<-- "examples/thermometer_py/channels.py:indication"
```

The whole script is [`examples/thermometer_py/channels.py`]. With the simulated device running, it prints:

```text
--8<-- "docs/generated/thermometer/channels_py.txt"
```

## Where to go next

- [Errors](errors.md) starts the next device, a bench supply, and returns errors of the API's own and the common
  ones.
- [Traits in depth](../api/traits.md) covers sub-traits from other crates, sub-traits a firmware leaves out, and how
  a trait shows up in rustdoc.
- [Arrays](../api/arrays.md) covers arrays of properties and streams, and arrays addressed by a key instead of an
  index.

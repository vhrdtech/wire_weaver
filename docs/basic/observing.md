---
level: basic
---

# Watching a value

The thermometer's gain changes in two ways. A host writes it, and the device lowers it by itself when its amplifier
clips. A host that wants to know the gain at all times could read it again and again, but most reads would return
the same value, and a change between two reads would show up late. Instead, the host can [[observe]] the gain: it
asks once, and the device sends an update whenever the gain changes. This chapter observes the gain from Rust, from
the command line and from Python, and shows what the device does to send the updates.

Start the simulated device in a terminal of its own, and leave it running while you read:

```sh
cargo run -p thermometer --example thermometer_sim
```

## Marking a property observable

A host can observe only a property that the API marks with `+observe` after its access. The thermometer's root
trait, shown in [Properties](properties.md#declaring-a-property), declares the gain this way:
`property!(rw+observe gain: Fixed<Ratio, One, u8, Range<1, 16>>)`. `mode` has no `+observe`, so a host can read it
but not observe it.

The mark is part of the API, because observing costs the device memory and code. A device keeps a little state per
observed property, and the generated code that sends updates exists only for marked properties.

## What the device does

A write from a host needs nothing from the device's code. After `set_gain` returns `SetResult::Set`, the generated
code marks the gain as changed, and the update goes out by itself.

A change the device makes by itself needs one call. The thermometer lowers its gain when a reading clips, and then
calls `mark_changed_gain()`, which `ww_codegen!` generates for every observable property:

```rust
--8<-- "examples/thermometer_device/src/lib.rs:overload"
```

The device keeps what hosts observe in an `ObserveState`: which properties, in which mode, and when each is due. The
generated code reaches it through one more handler:

```rust
--8<-- "examples/thermometer_device/src/lib.rs:observe_state"
```

Updates go out from the device's event loop. Each time the thermometer wakes up for its own work, it calls
`poll_properties`, which reads every property that is due through its read handler and sends the values:

```rust
--8<-- "examples/thermometer_device/src/lib.rs:poll"
```

`now` is the device's time, so `poll_properties` knows which modes are due. The loop also asks the `ObserveState`
when the next update is due, and wakes up then. [The thermometer's loop](../api/observing.md#the-server-loop) is one
function, `serve`, in [`examples/thermometer_device/src/lib.rs`].

## Watching from Rust

The generated [[client]] has `observe_NAME(mode)` for every observable property. `subscribe()` sends the request to
the device and returns an [[observer]], whose `recv()` waits for the next update. The program
[`examples/thermometer/examples/observe.rs`] observes the gain and then changes it twice:

```rust
--8<-- "examples/thermometer/examples/observe.rs:subscribe"
```

The [[observe mode]] `on_change(Duration::ZERO)` asks for every change, as soon as it happens. The first update
arrives right away with the value the gain has now, so the host has a value to start from. The program then writes
16, and makes the device lower the gain by taking a reading:

```rust
--8<-- "examples/thermometer/examples/observe.rs:changes"
```

Run it in a second terminal, while the simulated device runs in the first:

```sh
cargo run -p thermometer --example thermometer_observe
```

It prints a line per step, with the seconds since the program started:

```text
--8<-- "docs/generated/thermometer/observe.txt"
```

The figure draws these lines on a timeline. The gain on the device changes three times, and the host gets one update
per change, the last one caused by the device itself.

--8<-- "docs/generated/thermometer/observe_timeline.svg"

## Modes

The mode says when the device sends an update. Each observed property has a mode of its own.

| Mode                          | The device sends the value                                     | Use it for                       |
|-------------------------------|----------------------------------------------------------------|----------------------------------|
| `on_change(min)`              | when it changes, at most once per `min`                        | a setting, a state, a fault      |
| `periodic(every)`             | every `every`, changed or not                                  | a plot of a measured value       |
| `on_change_or_beat(min, max)` | when it changes, and at least once per `max` without a change  | a display that must stay current |

`min` keeps a value that changes often from flooding the link: with `on_change(Duration::from_millis(100))`, the host
gets at most ten updates per second, the last one always carrying the newest value. Every mode sends the value once
right after the request. The device has a budget of bytes per second for all updates, and it can raise a period that
is too short; [Observing in depth](../api/observing.md#budget) covers both.

## From the command line

`ww observe` watches every observable property of a level, on every change, until you press Ctrl+C. An empty path
is the API's root, the level of the gain:

```sh
ww --websocket ws://127.0.0.1:8080/ww observe
```

It prints one line per update, with the values the update carries, the updates per second and the bytes per second
they cost on the link. `--rate MS` asks for periodic updates instead.

## From Python

The Python module observes a level of the API, here its root, `dev.api`. A dictionary names the properties and their
modes, and `None` asks for every change:

```python
--8<-- "examples/thermometer_py/observe.py:observe"
```

`recv()` returns the next update as a dictionary. Its `"values"` hold the properties the update carries, by name.
The whole script is [`examples/thermometer_py/observe.py`]. With the simulated device running, it prints:

```text
--8<-- "docs/generated/thermometer/observe_py.txt"
```

The gain is a `Quantity` without a unit, stored as a `u8`.

## Where to go next

- [Streams and sinks](streams.md) sends every reading to the host, not only the latest value of a property.
- [Observing in depth](../api/observing.md) covers budgets, observing a whole level in one request, and every sample
  of a value that changes faster than the link can carry.

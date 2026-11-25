---
level: basic
---

# Numbers that know their unit

The thermometer has three kinds of numbers: a sampling rate in hertz, an amplifier gain without a unit, and
temperatures. A plain `u16` named `rate_hz` keeps the unit in its name, where no compiler, tool or script can read
it. WireWeaver puts the unit into the type instead. This chapter shows such a type on the thermometer's rate, how to
make values of it, why it costs nothing on the wire, and how tools and Python use it.

The chapter uses the thermometer example from [Properties](properties.md). Its first part needs no device; the
Python part at the end needs the simulated device running in a terminal of its own:

```sh
cargo run -p thermometer --example thermometer_sim
```

## A number with its unit in the type

The thermometer's API crate names the type of its sampling rate `Rate`:

```rust
--8<-- "examples/thermometer_api/src/lib.rs:rate"
```

`Fixed<Hertz, One, u16, Range<1, 1000>>` is a [[Fixed]] number: an integer whose meaning is spelled out by its four
type parameters. The table reads them for `Rate`.

| Parameter | In `Rate`         | Says                                                                    |
|-----------|-------------------|-------------------------------------------------------------------------|
| unit      | `Hertz`           | what the number measures                                                |
| step      | `One`             | the value of one count of the integer: 1 Hz                             |
| storage   | `u16`             | the integer that holds the count, in memory and on the wire             |
| bounds    | `Range<1, 1000>`  | the smallest and the largest count the device accepts, both included    |

The [[step]] is what turns the integer into a value. With `Milli` instead of `One`, the same `u16` would count
millihertz, and 250 would be 0.25 Hz. The step is the device's real resolution: the thermometer's sampler
can only run at whole hertz, so its step is `One`. The [[bounds]] are the same as in [Properties](properties.md):
`Rate` refuses 0 Hz and anything above 1000 Hz.

Every number of the thermometer's API is typed this way. `Gain` is `Fixed<Ratio, One, u8, Range<1, 16>>`, where
`Ratio` is the unit of a number without a dimension. A temperature is a point on the Celsius scale, which
[Temperatures and timestamps](temperatures.md) explains.

## Making a value

The program [`examples/thermometer/examples/units.rs`] works with the thermometer's numbers without a device. Its
first part makes a rate and fails to make another:

```rust
--8<-- "examples/thermometer/examples/units.rs:rate"
```

There are two ways to make a `Rate`. `Rate::bounded(250)` is for a value written in the source: in `const` code a
value outside the bounds fails the build, and elsewhere it panics. `Rate::try_new(2000)` is for a value that comes
from outside, such as a user, a file or another device. It returns an error that names the value, the bounds and the
type, so the program can handle it. `rate.get()` gives the stored integer back, and printing a rate shows it with its
unit.

## The same bytes as a bare number

The unit, the step and the bounds exist only in the type. On the wire a `Fixed` is exactly its storage integer, so a
typed API costs no byte more than one of bare integers. The program's second part writes a rate of 250 Hz and a bare
`u16` of 250 into bytes:

```rust
--8<-- "examples/thermometer/examples/units.rs:bytes"
```

The rate of 250 Hz is two bytes, the low byte first:

--8<-- "docs/generated/thermometer/rate_bits.md"

The bare `u16` of 250 is the same two bytes:

--8<-- "docs/generated/thermometer/u16_bits.md"

The device's code is the same too: a handler that takes a `Rate` reads one `u16` and compares it with 1 and 1000.
What the type adds is the check and the meaning. The device refuses a count outside the bounds while it decodes a
request, before any handler runs. The host's tools read the unit from the device and show `250 Hz` instead of `250`.

Run the program to see both parts:

```sh
cargo run -p thermometer --example thermometer_units
```

It prints the lines below, and then the lines of the [next chapter](temperatures.md):

```text
--8<-- "docs/generated/thermometer/units.txt"
```

## Choosing the unit, the step and the storage

Each number in an API answers three questions: what it measures, how fine the device resolves it, and how large it
gets. The unit answers the first and the step the second. The storage is the smallest integer that holds the largest
value in steps of that size. A timeout of up to a minute in milliseconds fits a `u16`, because a `u16` counts to
65 535. A float is the storage only for a value that has no fixed resolution.

The `ww_si` crate has a unit for every physical quantity, and short names for the common combinations. The table
lists the types for the numbers most devices have.

| A number of                    | Type                                          | On the wire |
|--------------------------------|-----------------------------------------------|-------------|
| a timeout up to 65 s           | `Millis<u16>`                                 | 2 bytes     |
| a duration up to 71 minutes    | `Micros<u32>`                                 | 4 bytes     |
| a frequency                    | `Fixed<Hertz, One, u16, Range<1, 1000>>`      | 2 bytes     |
| a voltage in millivolts        | `Fixed<Volt, Milli, i16>`                     | 2 bytes     |
| a percentage                   | `Fixed<Ratio, Centi, u8, Range<0, 100>>`      | 1 byte      |
| a buffer size in kibibytes     | `Fixed<Byte, Kibi, u16>`                      | 2 bytes     |
| a count of things              | `Fixed<Item, One, u32>`                       | 4 bytes     |
| a temperature                  | `Point<Celsius, Fixed<Kelvin, Centi, i16>>`   | 2 bytes     |

`Millis<u16>` is `Fixed<Second, Milli, u16>` under a short name. Two kinds of numbers are not `Fixed` at all. An
identifier, such as the number of a scale, is a newtype: `pub struct ScaleId(u8);`, derived with
`#[derive_shrink_wrap]`. A position in a table or a sequence number has types of its own, which the
[next chapter](temperatures.md) shows on the thermometer's samples.

A bare integer stays bare only with a reason, such as a register value that the API passes through unchanged.
`ww api check` points at a bare integer whose name ends in a unit, such as `timeout_ms` or `rate_hz`.

## From the command line

The [`ww` command line tool](../cli.md) reads the unit and the bounds from the device's [[introspection]] data. In
the listing of the thermometer's API, the rate shows its unit and its bounds in brackets after its type:

```ww
--8<-- "docs/generated/thermometer/introspect.txt"
```

Line 0 is `set_rate`. `[Hz, 1..=1000]` says that its argument counts hertz from 1 to 1000. `gain` on line 3 shows
only its bounds, because its unit `Ratio` has no symbol.

## From Python

The Python module turns a `Fixed` into a `ww.Quantity`, a number that knows its unit. A script passes any quantity
of the right unit, in any prefix, and the module converts it exactly into the stored count:

```python
--8<-- "examples/thermometer_py/units.py:rate"
```

0.5 kHz is stored as 500 steps of 1 Hz. 2 kHz is outside the bounds, so the module raises a `ValueError` before
anything is sent. A quantity read from the device keeps its stored count and its bounds:

```python
--8<-- "examples/thermometer_py/units.py:gain"
```

The whole script is [`examples/thermometer_py/units.py`]. With the simulated device running, it prints:

```text
--8<-- "docs/generated/thermometer/units_py.txt"
```

The last two lines come from the temperature, the subject of the next chapter.

## Where to go next

- [Temperatures and timestamps](temperatures.md) reads a value on a scale, such as 21.5 °C, and gives a reading the
  time it was taken.
- [Units in depth](../std_library/exact_units.md) covers the rest: units whose prefix each device picks at run time,
  units of your own, and arithmetic between units.

---
level: basic
---

# exact_units in an API

[`exact_units`](https://exact.vhrd.tech/) puts the unit, the scale and the storage of a number into its type,
`Fixed<U, P, T, B>`: a `T` in unit `U` and scale `P`, within the bounds `B`. On the wire and in memory it is exactly
the `T`: `shared_us: u64` becomes `shared: Fixed<Second, Micro, u64>` (or `Micros<u64>`) with the same bytes, and
introspection shows µs instead of a field name to trust. `Micros::<u32>::new(1500)` is 4 bytes on the wire,
`dc 05 00 00`, the bytes of the `u32` 1500.

The crate lives in its own repository, [romixlab/exact_units](https://github.com/romixlab/exact_units), with its
own book at [exact.vhrd.tech](https://exact.vhrd.tech/): it doesn't depend on WireWeaver, the wire format is its
optional `shrink_wrap` feature. [ww_si](ww_si.md) is built on it and re-exports everything, so `ww_si::Fixed` and
`exact_units::Fixed` are the same type; an API that already uses `ww_si` writes `ww_si::..`. For `Fixed` itself —
scales, storage, bounds, arithmetic, conversions, rounding, aliases, serde — see the book's
[first steps](https://exact.vhrd.tech/start/first_steps/) and [doing maths](https://exact.vhrd.tech/maths/arithmetic/).

## Fixed or Quantity

|                         | `Fixed<U, P, T, B>`                       | `ww_si::Quantity<U>`                        | `ww_si::AnyQuantity`            |
|-------------------------|-------------------------------------------|---------------------------------------------|---------------------------------|
| unit                    | in the type                               | in the type                                 | on the wire (an `SIExpr`)       |
| prefix and number type  | in the type, chosen by the API            | on the wire, chosen per device and value    | on the wire                     |
| bytes                   | the number alone                          | a prefix and type byte, then the number     | the number, prefix and the unit |
| on the device           | plain integer arithmetic                  | `ww_numeric::NumericValue`                  | the same                        |

Take `Fixed` when the API fixes the representation anyway (a timeout in ms as `u16`, a timestamp in µs as `u64`, a
buffer size in bytes): no byte for the prefix, and a `u64` field doesn't turn on `ww_numeric`'s 64-bit types for the
whole firmware. Take `Quantity` when each device should pick its own resolution (an ADC in mV on one board, in µV on
another). `Quantity::to_fixed` and `Quantity::from_fixed` convert between them.

## Aliases

`Secs`, `Millis`, `Micros`, `Nanos`, `Minutes`, `Hours`, `Days`, `Hz`, `Bytes`/`Kibibytes`/`Mebibytes`/`Gibibytes`,
`Count<Of, T>`, `Percent`/`Permille`/`Ppb`, `Duty16`, the Q formats and the binary angles are all `Fixed` under
another name. A moment in POSIX time is a `Point` of `origins::UnixEpoch`, aliased `UnixSecs`/`UnixMillis`/
`UnixMicros`/`UnixNanos`. See the book's [aliases table](https://exact.vhrd.tech/start/first_steps/#aliases-for-the-common-ones).

## Arithmetic and conversions

A unit's kind (STD-68) keeps a voltage gain from converting into a duty cycle or a resistance from a
transimpedance, the compiler and the evolution checker alike: see the book's [Kinds of
quantity](https://exact.vhrd.tech/maths/kinds/). The same kind picks a gain's dB formula, 20·lg or 10·lg: [Decibels](https://exact.vhrd.tech/levels/decibels/).
Multiplying or dividing two `Fixed` values gives back the exact result unit and scale as a type, with checked,
saturating and wrapping forms for what doesn't fit: [Products and
requantization](https://exact.vhrd.tech/fixed_point/products/).

## In an API

`Fixed` goes into properties, method arguments, `Timed` values and `Series` like any type. The bounds may be left out
in Rust source; the walker fills in `Unbounded`, and the API model, snapshots, `ww`, Python and the scaffold always
show all four arguments:

```rust
use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::Series;
use ww_si::{Fixed, Micros, Range, Timed, prefix::{Milli, One}, units::{Byte, Second}};

#[ww_trait]
trait Device {
    property!(rw timeout: Fixed<Second, Milli, u16>);
    property!(rw window: Fixed<Second, Milli, u16, Range<1, 5000>>);
    fn sizes() -> Series<'i, Timed<Fixed<Byte, One, u32>>>;
    fn ticks() -> Series<'i, Timed<Micros<u32>>>;
}
```

A `Series` of `Fixed` values has the same columns and bytes as a `Series` of the plain numbers.

## In `ww`, Python and dynamic clients

A client that only has a device's introspection data knows a `Fixed` for what it is: its unit, its scale, the number
type it is stored in and its bounds. `ww introspect` (and `ww api tree` from source) shows what the numbers are in
and the bounds after the type:

```
├─ 5 rw property timeout: Fixed<Second, Milli, u16, Unbounded> [ms]
├─ 7 rw property window: Fixed<Second, Milli, u16, Range<1, 5000>> [ms, 1..=5000]
├─ 15 rw property uptime: Fixed<Second, Micro, u64, Unbounded> [µs]
├─ 16 rw property buffer: Fixed<Byte, Kibi, u16, Unbounded> [KiB]
```

`ww observe`, `ww stream`, `ww tap` and `ww explain` print a value as the typed `Fixed` prints itself, exactly:
`1730000000000000 µs` from a `u64`, never through a float, `4 KiB` for a binary scale, the scale's own symbol for a
`scale!` of the API's crate. Values written as expressions (`ww explain 'uptime = q!(5 ms)'`, the dynamic client's
`expr::parse_value`) take a quantity in `q!(..)` and convert it exactly into the stored scale and number type:
`q!(5 ms)` into the `uptime` above stores 5000, `q!(0.1 ms)` 100, `q!(1 MiB)` into `buffer` 1024, `q!(8192 B)` 8.
What doesn't fit is refused before anything is sent, with the type named:

```
1.5 ns can't be written into Fixed<Second, Micro, u64, Unbounded>: not a whole number of µs
70 s can't be written into Fixed<Second, Milli, u16, Unbounded>: 70000 is out of range for u16
6 s can't be written into Fixed<Second, Milli, u16, Range<1, 5000>>: 6000 is out of 1..=5000
```

A plain number is the stored number (`uptime = 5000` is 5000 µs), what a script wrote into the field when it was a
plain `u64`, checked against the storage and the bounds the same way.

A float `Fixed` is never NaN unless its bound is `MaybeNan`, as in `exact_units` itself: every other bound,
`Unbounded` included, refuses NaN when a client writes it and when it comes off the wire. ±∞ is a value. A float
without a literal is written as its type's constant, `f32::NAN`, `f32::INFINITY` or `-f64::INFINITY`, which
`ww explain` also prints:

```
NaN is no value of Fixed<Volt, Milli, f32, Unbounded>: only a MaybeNan float holds NaN
```
[Indices and sequence numbers](indices.md) are shown with what they count, `#1000 (Sample)`,
`#2 (Item, from 1)`, `seq 41 (Frame)`, and take their number, an `IndexFrom1` refusing 0.

Python gets a `ww.Quantity`, as for a [`Quantity`](ww_si.md#in-ww-python-and-dynamic-clients), with the stored
number and type:

```python
u = dev.uptime.read()        # 1730000000000000 µs
u.value, u.prefix, u.storage # 1730000000000000, "Micro", "u64"
u.symbol, u.unit, u.fixed    # "s", "Second", True
u.to_si()                    # 1730000000.0
dev.window.read().bounds     # (1, 5000)
dev.uptime.write(dev.timeout.read())  # 5 ms, stored as 5000
dev.uptime.write(5000)       # as stored: 5000 µs
```

A write takes a `ww.Quantity` of a unit of the same dimension, converted exactly, or the stored number; anything
else is a `ValueError` with the same text as above. `float("nan")` into a NaN-free float `Fixed` is refused the
same way, and bytes holding NaN raise `ww.WireWeaverError` on decode.

In Rust, `wire_weaver_client::quantity::fixed_of(&ty, &bundle)` reads the type as a `FixedType` (unit, scale as a
`DynPrefix`, `Storage { base, bounds }`), `DynQuantity::from_value` reads a value of it, and `DynQuantity::to_value`
writes into it exactly or returns the error above. `quantity::DynCount` does the same for indices and sequence
numbers. The C consumer of the [conformance matrix](../serdes/conformance.md) reads a `Fixed` as its `T`.

## serde

With the feature `serde` the numbers are `Serialize` / `Deserialize`, `no_std` and without alloc (STD-94): the same
rule as the wire, each value as the number it stores, and a field marked `with_unit` as text in its unit (`"1.5
ms"`). See the book's [serde: configs and other formats](https://exact.vhrd.tech/wire/shrink_wrap/#serde-configs-and-other-formats).

## Units

The unit markers are grouped by cargo feature, each with its own dimension, symbol and exact scale; `Per`, `Times`
and `Sqrt` build the rest. See the book's [Units and dimensions](https://exact.vhrd.tech/start/units/).

The wire format is the feature `shrink_wrap`, on by default through `ww_si`. Without it `exact_units` has no
dependencies and its types are plain Rust (a `Dim`'s exponents are halves in an `i8`): code that only checks units at compile
time builds without the wire format, and the bytes are the same whichever crate turns it on.

---
level: basic
---

# exact_units: numbers with a fixed unit

`exact_units` puts the unit, the scale and the storage of a number into its type. `Fixed<U, P, T, B>` is a `T` in
unit `U` and scale `P` (a prefix like µ, a binary prefix like Ki, a power of ten, a fraction), within the bounds `B`.
On the wire and in memory it is exactly the `T`: `shared_us: u64` becomes `shared: Fixed<Second, Micro, u64>` (or
`Micros<u64>`) with the same bytes, and introspection shows µs instead of a field name to trust.

```rust
use exact_units::{Fixed, Micros, prefix::Milli};

let t: Micros<u32> = Fixed::new(1500);
assert_eq!(t.to_string(), "1500 µs");
assert_eq!(format!("{t:#}"), "0.0015 s");        // in the unit itself
assert_eq!(t.to::<Milli, u16>(), None);          // 1500 µs isn't a whole number of ms
assert_eq!((t * 2).to::<Milli, u16>(), Some(Fixed::new(3)));
assert_eq!(core::time::Duration::from(t), core::time::Duration::from_micros(1500));
```

`Micros::<u32>::new(1500)` is 4 bytes on the wire, `dc 05 00 00`, the bytes of the `u32` 1500.

The crate lives in its own repository, [romixlab/exact_units](https://github.com/romixlab/exact_units): it doesn't depend on WireWeaver, the wire
format is its optional `shrink_wrap` feature. It is `no_std` and needs no allocator. [ww_si](ww_si.md) is built on it and re-exports everything, so
`ww_si::Fixed` and `exact_units::Fixed` are the same type; an API that already uses `ww_si` writes `ww_si::..`.

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

## Scales

`P` is a marker type from `exact_units::prefix`:

- the SI prefixes `Quecto` .. `Quetta`, and `One` for the unit itself;
- the binary prefixes `Kibi`, `Mebi`, `Gibi`, `Tebi`;
- `Pow10<E>`, a power of ten without a name: tenths of a millisecond are `Fixed<Second, Pow10<-4>, u32>`, shown as
  `15·10⁻⁴ s`;
- `Pow2<E>`, a power of two: the Q formats and binary angles;
- `Rational<NUM, DEN>` (`u64`), a fraction: `Duty16` is `Fixed<Ratio, Rational<1, 65535>, u16>`, where 65535 is 100 %;
- `FullScale<V, BITS>` and `FullScaleMax<V, BITS>`, one step of a converter whose `2^BITS` codes (or largest code)
  span `V`.
- `Product<A, B>` and `Quotient<A, B>`, the scale of a product or quotient of two `Fixed` values: mV times mA is a
  `Product<Milli, Milli>`, a step of 1 µW.

Every scale is a [`Magnitude`](magnitude.md), a canonical product of prime powers with powers of π, ln 2 and ln 10,
so `Milli`, `Pow10<-3>` and `Rational<1, 1000>` are the same scale. A conversion with `to::<P2, T2>()` is exact or
refused (`None`), never rounded through a float, and its factor is worked out at compile time: one that isn't a
fraction of integers, or whose multiply doesn't fit, doesn't compile. 4 KiB as `Fixed<Byte, Kibi, u16>` is
`Fixed<Byte, One, u32>` 4096. The π power makes angles exact too: a degree is π/180 rad and a turn 2π rad.

## Storage

`T` is `u8` .. `u64`, `i8` .. `i64`, `f32`, `f64`, or one of `shrink_wrap`'s variable-length integers `UNib32`,
`UVlq32`, `UVlq64` and `Nibble` (trait `Storage`, `UnsignedStorage` for the unsigned ones).

## Bounds

`B` is `Unbounded` unless given, and then stays out of the way: `Fixed::new`, `+`, `-`, `*` and `/` by `T` exist for
the three-parameter form. Bounded values are made with `try_new` or `bounded` and checked on every read from the
wire, so a `set` of a bounded property out of range is refused before the handler runs:

```rust
use exact_units::{Fixed, Range, prefix::Milli, units::Second};

type Timeout = Fixed<Second, Milli, u16, Range<1, 5000>>;
assert!(Timeout::try_new(10).is_ok());
assert!(Timeout::try_new(0).is_err());
const LONG: Timeout = Timeout::bounded(5000);     // out of bounds here is a compile error
```

`Range<MIN, MAX>` is inclusive (`i64` bounds), `RangeU64` for big unsigned ones, `RangeDec<MIN, MAX, EXP>` decimal
bounds for floats (`RangeDec<-5, 5, -1>` is -0.5 ..= 0.5); NaN is always out of bounds. A bound that doesn't fit the
storage (`Range<-5, 300>` on a `u8`) doesn't build.

## Arithmetic and conversions

- `checked_add`, `checked_sub`, `checked_mul`, `checked_div` give `None` on overflow or out of bounds;
  `saturating_*` and `wrapping_*` (a device clock that wraps) as for the integers. `Wrapping` and `Saturating` as
  `B` make `+`, `-`, `*` and `/` do the same.
- `to` converts only exactly; `to_rounded`, `to_saturating`, `to_wrapping` and `requantize` name a rounding and an
  overflow policy, `into_rounded` and `From` are the infallible ones the bounds prove: see
  [Rounding and overflow](rounding.md).
- `PartialEq`, `Ord` and `Hash` compare the numbers; values of different scales are different types, convert them
  with `to` first. `==` and `<` also compare two `Fixed` of one unit, scale and storage with different bounds (a
  `Range`-bounded reading against an unbounded threshold); `Eq`, `Ord` and `Hash` stay within one type.
- Products and quotients of two `Fixed` (STD-83): `a * b` is a `Fixed<Times<U1, U2>, Product<P1, P2>, T>`, `a / b`
  a `Fixed<Per<U1, U2>, Quotient<P1, P2>, T>`, one multiply or division of `T` with the exact result scale as a type
  (mV · mA is a `Product<Milli, Milli>`, a step of 1 µW), overflow by the operands' policy. `checked_times`,
  `saturating_times`, `wrapping_times` and the `_per` twins take operands of any bounds; `times_into` / `per_into`
  give the result in another scale and storage, rounded once (mV · mA into mW). `convert` reaches the named unit
  (V · A to `Watt`).
- `core::time::Duration` both ways for s, ds, cs, ms, µs and ns: `Micros::<u32>::try_from(Duration::from_millis(3))`
  is 3000 µs, an error when it doesn't fit.
- `Display` as `1500 µs`, `{:#}` in the unit itself (`0.0015 s`); `Debug` as `Second(1500 µs u32)` (with the bounds
  when there are some: `Second(100 ms u16, 1..=5000)`); `defmt::Format` with the `defmt` feature.
- Units of one dimension but another kind of quantity don't convert (STD-68): a voltage gain `Per<Volt, Volt>` is
  not a current gain `Per<Ampere, Ampere>`, a power gain `Per<Watt, Watt>` or a `Ratio` (a duty cycle), and a
  transimpedance `Per<Volt, Ampere>` is not an `Ohm` (resistance), nor `Per<Ampere, Volt>` a `Siemens`. A quotient
  of one dimension keeps what cancelled, a gain times a voltage is a plain voltage; a scale still converts within a
  kind (mV/V to V/V). Crossing is a named step that keeps the number: `as_ratio` / `as_gain::<Volt>()`,
  `as_resistance` / `as_transimpedance`, `as_conductance` / `as_transconductance`. The rules are in the `kind`
  module of `exact_units`.
- A gain in dB by its kind (std): `Fixed::<Per<Volt, Volt>, One, f64>::new(10.0).to_db()` is 20 dB (20·lg for
  V/V, A/A, Pa/Pa), a power gain W/W 10·lg; `from_db` the other way. A plain `Ratio` has no level form and doesn't
  compile there: 0.5 duty is not −3 dB.
- Cycles per second become hertz only through the named `to_hertz`: a cycle is a counted thing, not a dimensionless
  1 (see [dimensions](ww_si.md#dimensions)).
- A `Fixed` is an amount (5 K, 3 ms). A reading against a zero that isn't the unit's (25 °C, gauge pressure, a
  register's `raw / 256 + 25 °C`, a heading that wraps) is a `Point` of an origin: see [points](points.md).

## Aliases

| Alias             | Type                                                   |
|-------------------|---------------------------------------------------------|
| `Secs<T>`         | `Fixed<Second, One, T>`                                |
| `Millis<T>`       | `Fixed<Second, Milli, T>`                              |
| `Micros<T>`       | `Fixed<Second, Micro, T>`                              |
| `Nanos<T>`        | `Fixed<Second, Nano, T>`                               |
| `Minutes<T>`      | `Fixed<Minute, One, T>`                                |
| `Hours<T>`        | `Fixed<Hour, One, T>`                                  |
| `Days<T>`         | `Fixed<Day, One, T>`                                   |
| `Hz<T>`           | `Fixed<Hertz, One, T>`                                 |
| `Bytes<T>`        | `Fixed<Byte, One, T>`                                  |
| `Kibibytes<T>`    | `Fixed<Byte, Kibi, T>`                                 |
| `Mebibytes<T>`    | `Fixed<Byte, Mebi, T>`                                 |
| `Gibibytes<T>`    | `Fixed<Byte, Gibi, T>`                                 |
| `Count<Of, T>`    | `Fixed<Of, One, T>`                                    |
| `Percent<T>`      | `Fixed<Ratio, Centi, T>`                               |
| `Permille<T>`     | `Fixed<Ratio, Milli, T>`                               |
| `Ppb<T>`          | `Fixed<Ratio, Nano, T>`                                |
| `Duty16`          | `Fixed<Ratio, Rational<1, 65535>, u16>`                |
| `Q15<U>`          | `Fixed<U, Pow2<-15>, i16, Saturating>` (`U` = `Ratio`) |
| `Q31<U>`          | `Fixed<U, Pow2<-31>, i32, Saturating>`                 |
| `UQ16_16<U>`      | `Fixed<U, Pow2<-16>, u32, Saturating>`                 |
| `Bam16`           | `Fixed<Turn, Pow2<-16>, u16, Wrapping>`                |
| `Bam32`           | `Fixed<Turn, Pow2<-32>, u32, Wrapping>`                |

The Q formats saturate and the binary angles wrap in arithmetic; they are made with `Fixed::of`, see
[Rounding and overflow](rounding.md#the-policy-of-a-type).

A moment in POSIX time (`time_t`, journald's `__REALTIME_TIMESTAMP`) since the Unix epoch is a `Point` of
`origins::UnixEpoch` (see [points](points.md#origins)), aliased as `UnixSecs<T>`, `UnixMillis<T>`, `UnixMicros<T>`
and `UnixNanos<T>` (`T` defaults to `u64`), each a `Point<UnixEpoch, Secs<T>>` / `Millis<T>` / `Micros<T>` /
`Nanos<T>`.

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
else is a `ValueError` with the same text as above.

In Rust, `wire_weaver_client::quantity::fixed_of(&ty, &bundle)` reads the type as a `FixedType` (unit, scale as a
`DynPrefix`, `Storage { base, bounds }`), `DynQuantity::from_value` reads a value of it, and `DynQuantity::to_value`
writes into it exactly or returns the error above. `quantity::DynCount` does the same for indices and sequence
numbers. The C consumer of the [conformance matrix](../serdes/conformance.md) reads a `Fixed` as its `T`.

## Units

The unit markers are in `exact_units::units`, grouped by cargo features a firmware turns on as it needs them
(`time`, `electrical`, `mechanical`, `angle`, `thermal`, `chemical`, `photometric`, `radiation`, `dimensionless`,
`information`, `counting`, `quantities` for all; `std` turns them all on). Each knows its dimension (`Dim`), symbol,
name and scale; products and quotients the compiler checks (`Volt * Ampere` is `Watt`), `Per<A, B>`, `Times<A, B>`
and `Sqrt<A>` build the others: a noise density is `Per<Volt, Sqrt<Hertz>>`, shown `V/√Hz`. Angle,
information (bit) and counted things (`Sample`, `Packet`, an API's own markers) are dimensions of their own; the
[ww_si page](ww_si.md#dimensions) explains why and what it changes against plain SI, and how a unit known only at run
time travels as an `SIExpr`.

The wire format is the feature `shrink_wrap`, on by default. Without it `exact_units` has no dependencies and its
types are plain Rust (a `Dim`'s exponents are `i8`): code that only checks units at compile time builds without
the wire format, and the bytes are the same whichever crate turns it on.

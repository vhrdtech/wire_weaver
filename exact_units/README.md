# exact_units

![Crates.io Version](https://img.shields.io/crates/v/exact_units)
[![CI](https://github.com/vhrdtech/wire_weaver/actions/workflows/ci.yml/badge.svg)](https://github.com/vhrdtech/wire_weaver/actions/workflows/ci.yml)

> Units of measure fixed at compile time and exact: a number with its unit, scale and storage in the type, `no_std`
> and no alloc.

```rust
use exact_units::{Fixed, Micros, prefix::Milli, units::Second};

let t: Micros<u32> = Fixed::new(1500);
assert_eq!(format!("{t}"), "1500 µs");
assert_eq!(format!("{t:#}"), "0.0015 s");
// exact conversions only: 1500 µs is not a whole number of ms
assert_eq!(t.to::<Milli, u16>(), None);
assert_eq!((t * 2).to::<Milli, u16>(), Some(Fixed::new(3)));
```

## What's in it

- `Fixed<U, P, T, B>`: a `T` in unit `U` and scale `P` (a prefix, a binary prefix, a power of ten, a fraction), within
  bounds `B`. In memory and on the wire exactly the `T` it replaces (`#[repr(transparent)]`). Conversions between
  scales are exact or refused, never rounded through a float; checked, wrapping and saturating arithmetic;
  `core::time::Duration` both ways; `Display` as `1500 µs`.
- Unit markers (`units::Volt`, `units::Per<Byte, Sample>`, `units::Sqrt<Hertz>`) with their dimension and scale, and
  the products and quotients the compiler checks (`Volt * Ampere` is `Watt`). Angle, information (bit) and counted
  things (`Sample`, `Packet`, an API's own markers) are dimensions of their own, so bytes never turn into % or rad
  and a sample rate is not a frequency.
- `Dim`: the dimension as data (SI base exponents, rad, bit, up to four counted bases, a square root).
- `Magnitude`: an exact factor of prime powers and powers of π, ln 2 and ln 10 (° is π/180 rad, a turn 2π rad, a
  decibel ln10/20 Np), always canonical, so two readers compute the same value and `Milli` equals `Rational<1, 1000>`.
- `SIExpr`: a unit as a tree, for a unit only known at run time.

## Features

| Feature        | What it adds                                                                                          |
|----------------|-------------------------------------------------------------------------------------------------------|
| none           | everything above as plain Rust types: `Dim` constants, unit markers, `Magnitude`, `Fixed` and its ops |
| unit groups    | `time`, `electrical`, `mechanical`, `angle`, `thermal`, `chemical`, `photometric`, `radiation`, `dimensionless`, `information`, `counting`; `quantities` for all |
| `shrink_wrap`  | the wire format: `SerializeShrinkWrap` / `DeserializeShrinkWrap` for every type, `SIExpr`, `Fixed` over `UNib32` / `UVlq*` / `Nibble` |
| `std`          | every unit group; with `shrink_wrap` the owned wire mirrors `DimOwned`, `CountedOwned`, `SIExprOwned` for hosts |
| `defmt`        | `defmt::Format`                                                                                       |

A firmware turns on the unit groups it uses.

## Where used

The types go on the wire in [WireWeaver](https://github.com/vhrdtech/wire_weaver) device APIs through its
`shrink_wrap` format, and [ww_si](https://crates.io/crates/ww_si) builds its run-time quantities (a prefix and number
type chosen per value) on top of this crate. Docs: https://vhrdtech.github.io/wire_weaver/std_library/exact_units/

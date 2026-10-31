---
level: advanced
---

# Points and origins

A [`Fixed`](exact_units.md) is an amount: 5 K warmer, 3 ms later, 20° further round. A `Point` is a reading on a scale
that starts somewhere: 25 °C, 1013 hPa absolute, a heading. `exact_units` has one affine algebra for all of them:
`Point<O, Fixed<U, P, T, B>>`, a number in steps `P` of the unit `U`, measured from the origin `O`.

```rust
use exact_units::{Fixed, Point, origins::{Celsius, Fahrenheit}, prefix::{Centi, Rational}, units::Kelvin};

type DegC = Point<Celsius, Fixed<Kelvin, Centi, i16>>;
type DegF = Point<Fahrenheit, Fixed<Kelvin, Rational<5, 9>, i16>>;

let water = DegF::from_raw(212);
assert_eq!(water.to::<Celsius, Centi, i16>(), Some(DegC::from_raw(10000)));
assert_eq!(DegC::from_raw(2500).to_string(), "25 °C");
```

On the wire a point is exactly its number: `DegC` is an `i16`, the origin is only in the type.

## The algebra

| Expression              | Result                  |
|-------------------------|-------------------------|
| point + vector          | point                   |
| point − vector          | point                   |
| point − point           | vector (a `Fixed`)      |
| point + point           | doesn't compile         |
| point · x, point / x    | doesn't compile         |
| point < point           | compares the numbers    |

The vector of a point is a `Fixed` in the same unit and steps: 25 °C − 20 °C is `Fixed<Kelvin, Centi, i16>` of 500,
5 K. The operators behave like the storage's own (a panic on overflow in debug builds); `checked_add`, `checked_sub`
and `checked_diff` return `None` instead. A point whose `Fixed` has bounds (`Range<-400, 1250>`) keeps them: leaving
them panics, `try_from_raw` and the checked forms return the error or `None`, and decoding refuses a number outside.

## Origins

An origin is a zero-size marker that is never sent. It carries:

- its unit: `Kelvin` for every temperature scale, °F too (its numbers count steps of 5/9 K);
- its offset, an exact `Offset` from its reference's zero: °C is 273.15 K above 0 K, °F 45967/180 K (459.67 °R of
  5/9 K each). `Offset` is a sign and a [`Magnitude`](magnitude.md), so no offset is ever rounded;
- its reference: the unit's own zero (0 K, vacuum), or a zero only known at run time (the ambient pressure);
- its modulus: none, or a period for angles that wrap;
- how it is shown: the symbol, and the step the shown number counts (1 K for °C, 5/9 K for °F).

`exact_units::origins` has these:

| Origin                    | Unit   | Offset                | Notes                                         |
|---------------------------|--------|-----------------------|-----------------------------------------------|
| `AbsoluteZero`            | K      | 0                     | thermodynamic temperature                     |
| `Celsius`                 | K      | 273.15                | SI Brochure 2.3.4                             |
| `Rankine`                 | K      | 0                     | shown in steps of 5/9 K                       |
| `Fahrenheit`              | K      | 45967/180             | shown in steps of 5/9 K (NIST SP 811 B.8)     |
| `Vacuum`                  | Pa     | 0                     | absolute pressure                             |
| `Gauge`                   | Pa     | 0 from ambient        | converts only with the ambient pressure       |
| `AboveStandardAtmosphere` | Pa     | 101325                | gauge with the reference stated               |
| `Angle<Turn>`             | rev    | 0                     | wraps every turn                              |
| `Angle<Degree>`           | °      | 0                     | wraps every 360°                              |
| `Angle<Radian>`           | rad    | 0                     | wraps every 2π (float storages only)          |
| `UnixEpoch`               | s      | 0                     | 1970-01-01 00:00:00 UTC, POSIX time (feature `time`) |

A crate declares its own with `origin!`, from the unit's zero:

```rust
use exact_units::{origin, Magnitude, Offset, units::Kelvin};

origin! {
    /// Degrees Rømer: 0 °Rø is 7.5 °C below 0 °C, a degree is 40/21 K.
    pub struct Romer { unit: Kelvin, symbol: "°Rø", offset: Offset::ratio(53055, 200), step: Magnitude::ratio(40, 21) }
}
```

`step` defaults to 1, `reference` to the unit's zero and `modulus` to `Linear` (given in that order, after
`offset`). Every value is a literal form (`Offset::decimal(27315, -2)`, `Offset::int(n)`, `Offset::ratio(n, d)`,
`Magnitude::ratio(..)`), so WireWeaver reads an origin from source like the units and scales of `unit!` and
`scale!`. `Origin::DEF` holds all of it as data (`OriginDef`), for a client that knows the origin only from the API
model.

## Register formulas

A sensor or bus formula like `raw / 256 + 25 °C` is an origin of its own, declared above another:

```rust
use exact_units::{origin, Fixed, Offset, Point, origins::Celsius, prefix::{Centi, Rational}, units::Kelvin};

origin! {
    /// The LSM6DS temperature register: raw / 256 + 25 °C.
    pub struct Lsm6dsTemp { above: Celsius, offset: Offset::int(25) }
}
origin! {
    /// J1939 engine coolant temperature (SPN 110): 1 °C per bit, -40 °C offset.
    pub struct J1939Temp { above: Celsius, offset: Offset::int(-40) }
}

type Lsm = Point<Lsm6dsTemp, Fixed<Kelvin, Rational<1, 256>, i16>>;
assert_eq!(Lsm::from_raw(128).to_string(), "25.5 °C");
assert_eq!(Lsm::from_raw(64).to::<Celsius, Centi, i16>().map(Point::raw), Some(2525));
```

`above` takes the unit, reference, modulus and display of the parent and adds the offset: the register's raw value is
the field's type (`Lsm` on the wire is the register's `i16`), and the formula lives in the type, shown in °C.

## Conversions

`to::<O2, P2, T2>()` gives the same point against another origin, in other steps and storage. The factor and the
offset are worked out at compile time; at run time it is a multiply, an add and a divide by constants (only the add
between origins of the same steps, K to °C). It is exact or `None`, never rounded: `None` when the result isn't a
whole number of `P2` for an integer `T2`, or doesn't fit `T2`.

```rust
use exact_units::{Fixed, Point, origins::{AbsoluteZero, Celsius}, prefix::{Centi, Milli}, units::Kelvin};

type DegC = Point<Celsius, Fixed<Kelvin, Centi, i16>>;
type MilliK = Point<AbsoluteZero, Fixed<Kelvin, Milli, u32>>;

assert_eq!(DegC::from_raw(2500).to::<AbsoluteZero, Milli, u32>(), Some(MilliK::from_raw(298_150)));
assert_eq!(MilliK::from_raw(298_155).to::<Celsius, Centi, i16>(), None); // not whole in 0.01 °C
```

Only origins with the same unit and reference convert. Gauge and absolute pressure have different references, so
`Point<Gauge, ..>::to::<Vacuum, ..>()` doesn't compile; `against` measures it against a zero given at run time:

```rust
use exact_units::{Fixed, Point, origins::{Gauge, Vacuum}, prefix::Hecto, units::Pascal};

let tyre = Point::<Gauge, Fixed<Pascal, Hecto, i32>>::from_raw(2200);
let ambient = Point::<Vacuum, Fixed<Pascal, Hecto, i32>>::from_raw(1013);
assert_eq!(tyre.against(ambient), Some(Point::from_raw(3213)));
```

`AboveStandardAtmosphere` states its reference (101325 Pa), so it converts to `Vacuum` at compile time.

## Angles that wrap

An origin with a modulus keeps its points within one period: `[0, M)` for an unsigned storage, `[-M/2, M/2)` for a
signed one. Point − point is the shortest way round, signed, and exactly half a turn is −M/2.

```rust
use exact_units::{Bam16, Fixed, Point, origins::Angle, prefix::Centi, units::{Degree, Turn}};

type Heading = Point<Angle<Turn>, Bam16>;
let a = Heading::from_raw(0xF000);
let b = Heading::from_raw(0x1000);
assert_eq!((b - a).get(), 0x2000); // an i16: the difference is signed

type CentiDeg = Point<Angle<Degree>, Fixed<Degree, Centi, u16>>;
assert_eq!((CentiDeg::from_raw(35000) + Fixed::new(2000)).raw(), 1000);
```

A period of exactly 2^BITS steps (a binary angle, `Bam16`) wraps in the storage's own wrapping arithmetic; any other
(36000 steps of 0.01°) by `rem_euclid` in a wider integer. A period that isn't a whole number of steps doesn't compile.
An encoder's index or a heading from north is an origin of its own, with `modulus: Modulo<..>` and its own reference.

## Firmware

A point costs what its number costs: the same size (`size_of::<DegC>()` is 2), the same bytes, and operators that
compile to the plain integer code. Instructions per function from the ELF (plain / `Point`, `=` where LLVM emits the
same code), thumbv6m (M0), thumbv7em-hf (M4F), thumbv8m.main-hf, measured on 28 Oct 2026:

| Operation                                     | M0 `3`  | M4F `3` | M4F `z` |
|-----------------------------------------------|---------|---------|---------|
| °C + vector, °C − °C (`i16`)                  | =       | =       | =       |
| `Bam16` + vector, − (wrapping)                | =       | =       | =       |
| 0.01° + vector (`rem_euclid` 36000), −        | =       | =       | =       |
| mK `u32` → 0.01 °C `i16`                      | —       | 34 / 34 | 22 / 22 |
| LSM6DS raw → 0.01 °C (`raw · 100 / 256 + 2500`) | 18 / 22 | 12 / 15 | —       |

The LSM6DS conversion is a few instructions longer than the hand-written one: it checks that the result is whole
and fits. The gate that keeps such pairs equal is TEST-29.

## On the wire and in the model

`Point` is `transparent`: its bytes are the `Fixed`'s, which are the plain number's (`Point<Celsius, Fixed<Kelvin,
Centi, i16>>` is the bytes of an `i16`). `Offset` is `final_structure`: the sign bit, then the `Magnitude` behind an option bit. Zero is one
byte (`00`), 273.15 twelve, −40 six, 45967/180 fifteen. A negative zero is not canonical and is refused on decode.
The cases are `stdlib.si.point.*` and `stdlib.si.offset.*` in the [conformance corpus](../serdes/conformance.md).

In the API model a point is an instance of `Point` with its origin as the first argument; the origin's definition
(`OriginDef`) goes into the model with INT-13.

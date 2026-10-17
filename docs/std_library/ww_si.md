---
level: basic
---

# ww_si

`ww_si` gives device APIs typed physical values: a quantity is an SI [prefix](#prefixes) plus a `ww_numeric`
`NumericValue` in whatever number type the device picks, with the unit in the type. The API says "this is a
voltage", each device decides whether that is a `u16` of mV or an `f32` of V, and the host reads them all.

```rust
use ww_si::{Ampere, Prefix, Volt, Watt};

let v = Volt::milli(3300u16);          // prefix Milli, value U16(3300)
let w = Volt::from_f32(3.3);           // prefix Unit, value F32(3.3)
assert_eq!(Volt::milli(1500u16), Volt::base(1.5f32)); // compares values, not representations
assert_eq!(v.to_f64(), 3.3);           // in V
assert_eq!(w.to_i128_in(Prefix::Milli), Some(3300));
assert_eq!(v.to_string(), "3300 mV");
let p: Watt = v * Ampere::milli(500u16);              // the compiler checks the units
```

`Volt::milli(3300u16)` is 3 bytes on the wire, `12 e4 0c`: the prefix and number type nibbles, then the `u16`.

## One type for every quantity

Every quantity is the same generic struct, and every unit a zero-size marker type in `ww_si::units`:

```rust
pub struct Quantity<U: Unit> {
    pub prefix: Prefix,
    pub value: NumericValue,
    _unit: PhantomData<U>,      // no bits on the wire, no bytes in RAM
}

pub type Volt = Quantity<units::Volt>;
pub type Ampere = Quantity<units::Ampere>;
// .. one alias per unit
```

APIs and code write the aliases (`property!(ro vbus: Volt)`, `Timed<Ampere>`). Everything below is written once for
all quantities, so `Hertz::kilo(400u32)` works the same way as `Volt::milli(3300u16)`.

A marker knows its unit through the `Unit` trait: its SI dimension (`Dim`, exponents of s, m, kg, A, K, mol, cd),
its name and symbol, and how a value becomes coherent SI (`Scale`: coherent, a power of ten, a factor, or a factor
and an offset for °C). `Volt::unit()` returns it; `ww_si::units::by_name("Volt")` finds it by the marker's name, which
is what a client that only sees introspection data uses.

## Features

Everything is a cargo feature, so a firmware compiles only what it uses. `std` (the default) enables all of them.

| Feature        | What                                                                                              |
|----------------|---------------------------------------------------------------------------------------------------|
| `time`         | `Second`, `Hertz`, `HertzPerSecond`, `Baud`, [`Timed<T>`](#timed-values)                          |
| `electrical`   | `Ampere`, `Volt`, `Ohm`, `Farad`, `Siemens`, `Henry`, `Coulomb`, `Watt`, `Weber`, `Tesla`         |
| `mechanical`   | `Meter`, `Gram`, `Newton`, `Pascal`, `Joule`, `Torque`, `Litre`, `VolumetricFlow`, `Speed` .. `Yank` |
| `angle`        | `Radian`, `Degree`, `Steradian`, `AngularVelocity`, `AngularAcceleration`                         |
| `thermal`      | `Kelvin`, `DegreeCelsius`                                                                         |
| `chemical`     | `Mole`, `Katal`                                                                                   |
| `photometric`  | `Candela`, `Lumen`, `Lux`                                                                         |
| `radiation`    | `Becquerel`, `Gray`, `Sievert`, `ElectronVolt`                                                    |
| `logarithmic`  | `DeciBel`, `DeciBelmW` (plain structs without a prefix)                                           |
| `dimensionless` | `Ratio`: duty, probability, scale (a group of its own, as it is used next to any other)          |
| `quantities`   | all of the groups above                                                                           |
| `host`         | host helpers: [`Unwrapper`](#second-as-a-timestamp)                                               |
| `u16`, `f32`, .. `all` | number types of `NumericValue`, passed on to `ww_numeric` (see [ww_numeric](ww_numeric.md)) |

```toml
# firmware with voltages and timestamps, stored as u16, u32 and f32
ww_si = { version = "0.2", default-features = false, features = ["time", "electrical", "u16", "u32", "f32"] }
```

The bytes on the wire are the same for every feature set.

## Constructors

| Constructor                                   | Value                                                        |
|-----------------------------------------------|--------------------------------------------------------------|
| `new(prefix, value)`                          | any enabled number type: `u16`, `f32`, `UVlq32`, ..          |
| `base`, `milli`, `micro`, `nano`, `pico`      | `Volt::base(3u8)` is 3 V, `Second::micro(100u32)` 100 µs      |
| `centi`, `kilo`, `mega`, `giga`               | `Hertz::kilo(400u32)` is 400 kHz, `Gram::kilo(5u8)` 5 kg      |
| `from_f32(x)`                                 | `f32` in the unit itself, `const`                            |
| `from_parts(prefix, value)`                   | `const`, for statics and tables                              |
| `quantize(x, prefix, storage)`                | an `f32` put into the representation an API documents        |
| `Ratio::percent`, `per_mille`, `ppm`          | `Centi`, `Milli`, `Micro`                                    |

Mass is in grams, so a prefix works on it like on any other unit: 5 kg is `Gram::kilo(5u8)`.

`quantize` is what a device does with a value it computed as a float: `Ampere::quantize(0.1523, Prefix::Milli,
NumericBaseType::I32)` is 152 mA, rounded to the nearest integer and saturated at the storage's range (a float storage
takes the value as is). It computes in `f32`, no `f64` code on a device.

## Conversions and comparisons

- `to_f64()` / `to_f32()`: the value in the unit itself (V, A, Hz, s, °, g);
- `to_f64_in(prefix)`: in another prefix, `Volt::from_f32(3.3).to_f64_in(Prefix::Milli)` is about 3300.0;
- `to_i128_in(prefix)`: as an integer, exact for integer storage (truncated toward zero when going to a bigger
  prefix), rounded for floats; `None` when it doesn't fit;
- `to_si_f64()`: in coherent SI through the unit's scale: 90 ° is 1.5708 (rad), 20 °C is 293.15 (K), 5 g is 0.005 (kg);
- `with_prefix(prefix)`: the same value in another prefix and the same number type, if that is exact
  (`Volt::milli(3000u16)` to `Unit` is 3 V; 3300 mV is `None`), for autoranging before sending;
- `same_repr(&other)`: same prefix and the same number in the same type, a cheap "did it change since I sent it".

`PartialEq` and `PartialOrd` compare values across prefixes and number types: `Volt::milli(1000u16) == Volt::base(1u8)`.
Two integers are compared exactly, even 60 orders of magnitude apart; otherwise through `f64`.

`Display` writes the number as stored with its prefix, `{:#}` in the unit itself; `Debug` adds the unit's name and
the number type, `defmt::Format` is like `Display`:

| Value                          | `{}`           | `{:#}`    | `{:?}`              |
|--------------------------------|----------------|-----------|---------------------|
| `Volt::milli(3300u16)`         | `3300 mV`      | `3.3 V`   | `Volt(3300 mV U16)` |
| `Torque::milli(250i32)`        | `250 mN·m`     | `0.25 N·m` |                    |
| `Gram::kilo(5u8)`              | `5 kg`         | `5000 g`  |                     |
| `Ratio::percent(50u8)`         | `50 %`         | `0.5`     |                     |

`Baud` is a symbol rate, the baud rate of a UART or a protocol decoder, kept apart from `Hertz` so a baud rate is
never taken for a clock frequency (and a tool shows `115200 Bd`, not `115.2 kHz`). `Torque` (N·m) has the dimension
of `Joule` but is kept apart the same way. `Ratio` is a plain fraction (0.5 is half; `percent` is `Centi`), for a duty
cycle, a probability or a scale factor; `DeciBel` is the logarithmic one.

## Unit algebra

`*` and `/` between quantities are checked by the compiler: only the products in the table below build, and the
result has the right type. `+` and `-` take two values of the same unit, a `Ratio` keeps the unit, and `* f32` /
`/ f32` scale a value.

```rust
use ww_si::{Ampere, AngularVelocity, Ratio, Torque, Volt, Watt};

let p: Watt = Volt::milli(3300u16) * Ampere::milli(500u16);   // 1.65 W
let p_mech: Watt = Torque::base(2u8) * AngularVelocity::base(100u8);
let i: Ampere = p / Volt::base(3.3f32);
let v_applied: Volt = Volt::base(48u8) * Ratio::percent(50u8); // 24 V
```

`Volt::base(1u8) * AngularVelocity::base(1u8)` and `Volt::base(1u8) + Ampere::base(1u8)` don't compile (the first
names the missing `UnitMul<AngularVelocity>` impl of `Volt`).

| `A * B = C` (also `B * A`, `C / A`, `C / B`) |                                           |
|-----------------------------------------------|-------------------------------------------|
| `Volt * Ampere = Watt`                        | `Torque * AngularVelocity = Watt`         |
| `Ampere * Ohm = Volt`                         | `Watt * Second = Joule`                   |
| `Ampere * Second = Coulomb`                   | `Farad * Volt = Coulomb`                  |
| `Volt * Second = Weber`                       | `Henry * Ampere = Weber`                  |
| `AngularVelocity * Second = Radian`           | `Speed * Second = Meter`                  |
| `Acceleration * Second = Speed`               | `Gram * Acceleration = Newton`            |
| `Newton * Meter = Joule`                      | `Hertz * Second = Ratio`                  |
| `HertzPerSecond * Second = Hertz`             |                                           |

Each line is checked against the SI exponents of its units when `ww_si` compiles, so the table can't be wrong; a
unit with an offset scale (°C) can't be in it. `Newton * Meter` is a `Joule` (work), a torque comes from `Torque`
values.

The numbers are computed in `f32` (in `f64` when an operand is stored as `F64`) through each unit's scale
(`Gram::kilo(2u8) * Acceleration::base(3u8)` is 6 N), and the result is stored as `F32` / `F64` without a prefix:
operators are for host and FPU math, the representation on the wire stays the device's choice through `quantize`.
They need the `f32` feature.

## Compound units and your own

A unit outside the list is written with `Per<A, B>` and `Times<A, B>` of markers. They are not normalized:
`Per<Meter, Second>` is not `Speed`, it is shown as `m/s` and has the same dimension.

```rust
use ww_si::Quantity;
use ww_si::units::{AngularVelocity, Per, Times, Torque};

/// Torque = b·ω.
let b = Quantity::<Per<Torque, AngularVelocity>>::from_f32(1.2e-4);
assert_eq!(b.to_string(), "0.00012 N·m/(rad/s)");
/// Propeller: torque = k·ω·|ω|.
let k = Quantity::<Per<Torque, Times<AngularVelocity, AngularVelocity>>>::base(2u8);
assert_eq!(k.to_string(), "2 N·m/((rad/s)·(rad/s))");
```

An API crate can declare its own marker: a field-less struct deriving `Copy, Clone, Debug, Default` and an `impl
Unit` with its dimension and `UnitExpr` (see the `Unit` rustdoc, a revolutions-per-minute example). It works with
every method above; it has no products, and clients that don't know it show it by name.

## `Second` as a timestamp

A `Second` that marks when something happened (an ADC sample, the start and end of an I2C transaction) is the
device's time since its clock started. A device has one clock, so the timestamps of all its traits line up, and
`ww_timebase::Clock::now()` reads it in full. The device picks the prefix and the storage, and keeps them the same
for a given stream or method.

Fixed-width unsigned storage holds the time modulo `2^bits`, so it wraps:

| Storage          | Wraps after (µs) | Wraps after (ms) |
|------------------|------------------|------------------|
| `U16`            | 65.5 ms          | 65.5 s           |
| `U32`, `UVlq32`  | 71.6 min         | 49.7 days        |
| `U64`, `UVlq64`  | 584 000 years    | never            |

Timestamps are never signed or floats. On the host, `Unwrapper` (feature `host`) gets the full time back. It is
anchored on a full-width reading of the same clock, then takes each timestamp as the value with the same low bits
closest to the previous one:

```rust
use ww_si::{Second, Unwrapper};

let mut unwrapper = Unwrapper::new(Second::micro(5_000_000_123u64)).unwrap(); // Clock::now()
let full = unwrapper.unwrap(Second::micro(705_032_827u32)).unwrap();        // U32 µs from a stream
assert_eq!(full, Second::micro(5_000_000_123u64));
```

That works while consecutive timestamps are less than half a wrap period apart; re-anchor with `anchor(now)` after
a longer pause. Use one `Unwrapper` per stream. `U64` and signed values pass through unchanged. The anchor and the
timestamps can be in different prefixes.

## Timed values

`Timed<T> { t: Second, value: T }` is a value with the time it was taken, `t` a device timestamp as above: a
sample of a property, an item of a stream (`Timed<Ampere>`). It is a `final_structure` struct of `t` and then
`value`, the same bytes a `#[derive_shrink_wrap(final_structure)]` struct of those two fields has, so no fields can
be added to it later. In an API it is used with its argument, `property!(ro current: Timed<Ampere>)`; its definition
is in the `ww_si` snapshot, so devices leave it out of their introspection data (see
[generic types in an API](../types.md#generic-types-in-an-api)).

```rust
use ww_si::{Ampere, Second, Timed};

let sample = Timed::new(Second::micro(1_000_000u32), Ampere::milli(-1200i32));
```

In a `Series` (see [series](../serdes/series.md)) `t` is a column like any other: 32 `Timed<Volt>` samples at a
40 µs clock are 22 bytes.

## On the wire and in introspection

A quantity is a `final_structure` struct of its prefix and its value; the unit takes no bits. The bytes are the same
as in ww_si 0.1, where each quantity was a struct of its own, so recorded data and `Series` blobs still decode:

| Value                        | Bytes                |
|------------------------------|----------------------|
| `Volt::milli(3300u16)`       | `12 e4 0c`           |
| `Ampere::micro(-152000i32)`  | `26 40 ae fd ff`     |
| `Volt::from_f32(3.3)`        | `07 33 33 53 40`     |

In the API model a use of `Volt` is an instance of the generic `Quantity` with the marker `units::Volt` as its
argument (an alias is not a definition of its own). Both are in the `ww_si` snapshot, so a device sends a reference
to them, not their definitions.

`BaseUnit` (a unit known only at run time, used by `ww_adc::ChannelInfo::unit`) is still the one from ww_si 0.1; a
run-time unit model that maps onto the markers replaces it later.

## Prefixes

`Prefix` covers every SI prefix from `Quecto` (10⁻³⁰) to `Quetta` (10³⁰), `From<Prefix> for i8` gives the power
of ten and `symbol()` its symbol (`m`, `µ`, `k`). The most used ones (`Unit`, `Milli`, `Micro`, `Nano`, `Pico`,
`Kilo`, `Mega`, `Giga`) take one nibble on the wire.

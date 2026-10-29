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
assert_eq!(w.to_i32_in(Prefix::Milli), Some(3300));
assert_eq!(v.to_string(), "3300 mV");
let p: Watt = v * Ampere::milli(500u16);              // the compiler checks the units
```

`Volt::milli(3300u16)` is 3 bytes on the wire, `12 e4 0c`: the prefix and number type nibbles, then the `u16`.

When the API fixes the prefix and number type anyway (a timeout in ms as `u16`, a timestamp in µs as `u64`), a
[`Fixed`](exact_units.md) has them in the type and sends the number alone: `Micros<u32>` is the 4 bytes of the `u32`.
The units, dimensions, `Fixed` and `SIExpr` live in the [exact_units](exact_units.md) crate, which `ww_si` builds on
and re-exports (`ww_si::units`, `ww_si::Dim`, `ww_si::Fixed` are its items); `ww_si` adds the run-time quantities:
`Quantity`, `AnyQuantity`, `Timed` and the timestamp tools. [Which one to pick](#quantity-fixed-or-anyquantity), and
how they convert into each other, is below.

## One type for every quantity

Every quantity is the same generic struct, and every unit a zero-size marker type in `ww_si::units`:

```rust
pub struct Quantity<U: Unit, B: QuantityBounds = Unbounded> {
    pub prefix: Prefix,
    pub value: NumericValue,
    _unit: PhantomData<U>,      // no bits on the wire, no bytes in RAM
    _bounds: PhantomData<B>,    // the allowed range, see "Bounds" below
}

pub type Volt = Quantity<units::Volt>;
pub type Ampere = Quantity<units::Ampere>;
// .. one alias per unit
```

APIs and code write the aliases (`property!(ro vbus: Volt)`, `Timed<Ampere>`). Everything below is written once for
all quantities, so `Hertz::kilo(400u32)` works the same way as `Volt::milli(3300u16)`.

A marker knows its unit through the `Unit` trait: its dimension (`Dim`, exponents of s, m, kg, A, K, mol, cd, rad,
bit and up to four counted bases, see [dimensions](#dimensions)), its name and symbol, and how a value becomes coherent SI (`Scale`: coherent, a power of ten, a factor, or a factor
and an offset for °C). `Volt::unit()` returns it; `ww_si::units::by_name("Volt")` finds it by the marker's name, which
is what a client that only sees introspection data uses.

## Quantity, Fixed or AnyQuantity

All three carry a value with its unit; they differ in what is fixed when the API is written and what travels with
every value:

| | Unit | Prefix and number type | On the wire | Pick it for |
|---|---|---|---|---|
| `Fixed<U, P, T, B>` | in the type | in the type | the number alone: `Micros<u32>` is 4 B | the API knows the scale and the storage: a timeout in ms as `u16`, a duty cycle in 1/65535, an ADC step |
| `Quantity<U, B>` | in the type | per value | a prefix and number type nibble, then the number: `Volt::milli(3300u16)` is 3 B | the API knows the unit, each device picks the scale and storage (`u16` of mV here, `f32` of V there) |
| `AnyQuantity` | per value | per value | the prefix, number and the whole unit: 19 B for 3300 mV | the unit is set at run time: configured sensor inputs, generic loggers |

The units are one system in all three: the markers of `ww_si::units` (from [exact_units](exact_units.md)), with real
dimensions. A value only converts into another of the same dimension: `Byte`, `Sample` and `Ratio` are each a
dimension of their own (information, a counted base, one) and never convert into each other, whatever their numbers.

Conversions, both ways and exact:

- `Quantity::try_from_fixed(f)` takes a `Fixed` as a `Quantity` with its prefix and number as they are: a named
  prefix where the scale has one (`Milli`, `Kibi`), else one of the [unnamed scales](#prefixes) `Pow10` (`Pow10<-4>`),
  `Pow2` (a Q format's 2⁻¹⁵) or `FullScale` (the 1/65535 of a `Duty16`). Any other scale (a BAM's π/2¹⁵, 1/5) is
  refused with `UnknownScale`: `AnyQuantity::from_fixed(f)` holds every `Fixed` exactly, the scale moved into its unit
  (`3·Bam16Rad·rad`). `Quantity::from_fixed(f)` is the infallible form for decimal prefixes.
- `q.to_fixed::<P, T, B>()` gives the `Fixed` back if the value is exact in scale `P`, fits `T` and is within `B`,
  else `None`: `Volt::milli(3300u16).to_fixed::<Milli, u16, Unbounded>()` is `Some`, to `Unit` it is `None`.
- `AnyQuantity::from(q)` puts the unit into the value; `AnyQuantity::to_quantity::<U>()` takes it out again when the
  unit is `U`'s ([below](#units-known-only-at-run-time)).

Adding, comparing and converting give the same result, or the same refusal, for a `Fixed` and the `Quantity` it
converts to: a property test runs both sides over every scale (`ww_si/tests/parity.rs`).

## Bounds

`B` is the range a value must be in, in the unit's coherent value, whatever prefix a value comes with: a supply rail
of 0 to 3.3 V is `Quantity<units::Volt, RangeDec<0, 33, -1>>`, and `3300 mV`, `3.3 V` and `3 V` are in it, `3301 mV`
is not. The markers are the ones `Fixed` uses (`Range`, `RangeU64`, `RangeDec`, `Unbounded`, the default).

```rust
use ww_si::{units, Prefix, Quantity, RangeDec};

type Rail = Quantity<units::Volt, RangeDec<0, 33, -1>>;
assert!(Rail::try_new(Prefix::Milli, 3300u16).is_ok());
assert!(Rail::try_new(Prefix::Milli, 3301u16).is_err());     // OutOfBounds
let v = Rail::try_new(Prefix::Unit, 3u8).unwrap().unbounded(); // a plain Volt again
```

A bounded quantity is made with `try_new`, or from a plain one with `bounded::<B>()`; the constructors without a check
(`milli`, `from_f32`, ..) are only on the unbounded `Quantity<U>`. Decoding checks the bounds too, also every item of
a `Series`: a device never sees an out-of-range value in a handler. The bounds are in the API model (`Unbounded`
too, named in every instance), so `ww` and the GUI show the limits of a `Quantity` the same way as of a `Fixed`.
Bounds cost no bytes on the wire: a bounded quantity has the bytes of the unbounded one.

## Features

Everything is a cargo feature, so a firmware compiles only what it uses. `std` (the default) enables all of them.

| Feature        | What                                                                                              |
|----------------|---------------------------------------------------------------------------------------------------|
| `time`         | `Second`, `Hertz`, `HertzPerSecond`, `Baud`, [`Timed<T>`](#timed-values)                          |
| `electrical`   | `Ampere`, `Volt`, `Ohm`, `Farad`, `Siemens`, `Henry`, `Coulomb`, `Watt`, `Weber`, `Tesla`         |
| `mechanical`   | `Meter`, `Gram`, `Newton`, `Pascal`, `Joule`, `Torque`, `Litre`, `VolumetricFlow`, `Speed` .. `Yank` |
| `angle`        | `Radian`, `Degree`, `Steradian`, `AngularVelocity`, `AngularAcceleration` (rad is a dimension)    |
| `thermal`      | `Kelvin`, `DegreeCelsius`                                                                         |
| `chemical`     | `Mole`, `Katal`                                                                                   |
| `photometric`  | `Candela`, `Lumen`, `Lux`                                                                         |
| `radiation`    | `Becquerel`, `Gray`, `Sievert`, `ElectronVolt`                                                    |
| `logarithmic`  | `DeciBel`, `DeciBelmW` (plain structs without a prefix)                                           |
| `dimensionless` | `Ratio`: duty, probability, scale (a group of its own, as it is used next to any other)          |
| `information`  | `Bit`, `Nibble` (4 bit), `Byte` (8 bit), `Halfword` (2 byte, 16 bit), `Word` (4 byte, 32 bit), `Doubleword` (8 byte, 64 bit) (bit is a dimension) |
| `counting`     | `Item`, `Sample`, `Frame`, `Packet`, `Cycle`, `Block`, `Sector`, `Chunk`, `Char`, `Code`: counted bases |
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
- `to_i32_in(prefix)` / `to_u32_in(prefix)`: as an integer, exact for integer storage (truncated toward zero when
  going to a bigger prefix), rounded for floats; `None` when it doesn't fit. `Second::milli(5u16).to_u32_in(Prefix::Micro)`
  is `Some(5000)`;
- `to_u32_wrapping_in(prefix)`: the same modulo 2^32, what a wrapping 32-bit counter of that unit shows, to compare a
  timestamp with a device clock;
- `to_i128_in(prefix)`: like `to_i32_in` up to `i128`, only with a 64- or 128-bit number type enabled (`std` has
  them);
- `to_si_f64()`: in coherent SI through the unit's scale: 90 ° is 1.5708 (rad), 20 °C is 293.15 (K), 5 g is 0.005 (kg);
- `with_prefix(prefix)`: the same value in another prefix and the same number type, if that is exact
  (`Volt::milli(3000u16)` to `Unit` is 3 V; 3300 mV is `None`), for autoranging before sending;
- `same_repr(&other)`: same prefix and the same number in the same type, a cheap "did it change since I sent it".

`PartialEq` and `PartialOrd` compare values across prefixes and number types: `Volt::milli(1000u16) == Volt::base(1u8)`.
Two integers are compared exactly, even 60 orders of magnitude apart; through `f64` when one is a float.

On a small MCU the integer conversions, comparisons and the exact arithmetic below take no wider arithmetic than the
enabled number types need: with types up to 32 bits (`u32`, `i32`, ..) everything is `u32` code, nothing of 64 or
128 bits is linked, and `ww_numeric::Integer` (sign and magnitude) does the work. The same prefix compares the stored
numbers; across prefixes the value with the bigger prefix is scaled by powers of ten with an overflow check, and an
overflow means it is the bigger one. On a Cortex-M0+ (Donguru, opt-level `z`) this took 6 KiB of flash off what
widening to `i128` cost.

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

### Exact integer arithmetic

For values of the same unit in integer storage, timestamps and durations above all, there are exact methods that
keep integer storage and never round:

| Method                    | Result                                                                                |
|---------------------------|---------------------------------------------------------------------------------------|
| `a.checked_add(&b)`       | `a + b` in the finer prefix of the two and `a`'s number type                          |
| `a.checked_sub(&b)`       | `a - b`, the same way                                                                 |
| `a.checked_mul_int(n)`    | `a * n` (any integer up to the widest enabled type) in `a`'s prefix and number type   |
| `a.wrapping_add(&b)`      | `a + b` in `a`'s prefix and number type, wrapping at its width like a device clock    |
| `a.wrapping_sub(&b)`      | `a - b`, the same way                                                                 |
| `t0.steps(&dt)`           | `t0`, `t0 + dt`, `t0 + 2·dt`, .. without end, each as `wrapping_add` computes it      |

The `checked_*` ones give `None` when the result doesn't fit the number type; all of them give `None` for floats
(their operators are `+`, `-`, `*`) and the `wrapping_*` ones and `steps` when `b` is not a whole number of `a`'s
prefix (40 µs added to a time in ms), or when `b` in `a`'s prefix is beyond the widest enabled integer type.

```rust
use ww_si::Second;

assert_eq!(Second::milli(5u32).checked_add(&Second::micro(40u16)), Some(Second::micro(5040u32)));
assert_eq!(Second::micro(u32::MAX).checked_add(&Second::micro(1u8)), None);
assert_eq!(Second::micro(u32::MAX).wrapping_add(&Second::micro(2u8)), Some(Second::micro(1u32)));
// the times of 32 samples at 40 µs, the first one taken at t0
let t0 = Second::micro(4_294_967_000u32);
let times: Vec<Second> = t0.steps(&Second::micro(40u8)).unwrap().take(32).collect();
```

A batched stream's generated `<name>_push_many(state, t0, dt, values)` stamps a block of samples this way.

## Dimensions

Besides the seven SI base dimensions, `Dim` has three kinds of its own, so the compiler and a client keep apart what
SI alone would call the same:

- **Angle (rad).** The plane angle is a base dimension: `AngularVelocity` is rad·s⁻¹ and not `Hertz` (s⁻¹),
  `Torque` is J/rad (kg·m²·s⁻²·rad⁻¹, written N·m), so `Torque * AngularVelocity = Watt` still balances and a torque
  isn't a joule. `Steradian` is rad², `Lumen` cd·rad².

  This departs from SI, where rad = 1, and has consequences: Hz and rad/s don't convert into each other by
  themselves, a code goes from one to the other with an explicit 2π (or through `Cycle` per second); a moment of
  inertia is kg·m²/rad² (`Per<Torque, AngularAcceleration>`), so torque over angular acceleration balances; an arc
  length r·θ carries rad (`Times<Meter, Radian>`), it isn't a plain length.
- **Information (bit).** `Bit` is the coherent unit; `Nibble` (4 bit), `Byte` (8 bit), `Halfword` (16 bit), `Word`
  (32 bit) and `Doubleword` (64 bit) are scales of it, like `Gram` to kilogram. `units::Nibble` is this unit, not
  `shrink_wrap::Nibble`, the 4-bit storage a `Fixed` may hold its number in. Bytes are never a `Ratio` or an angle,
  and `Per<Byte, Second>` is a data rate, not a frequency.
- **Counted bases.** `Item`, `Sample`, `Frame`, `Packet`, `Cycle`, `Block`, `Sector`, `Chunk`, `Char` and `Code` count
  things: each is a dimension of its own, so `Per<Sample, Second>` (s⁻¹·Sample) isn't `Hertz` and samples aren't
  packets. A base is known by its crate and marker name together (`Counted { crate_name, name, exp }`): two crates'
  `Sample` are different bases, and a marker moved to another module of its crate stays the same base. It is shown by
  the marker name alone. An API crate's own counted marker takes its dimension from `ww_si::counted!(Bucket)`, which
  fills in the crate. A dimension holds up to four of them (`MAX_COUNTED`), sorted by crate and name; a `Dim` on the
  wire whose bases are out of order, repeated or with a zero exponent is refused on decode, so every dimension has
  exactly one encoding. Bytes of a block are `Per<Byte, Block>`.
- **Square roots.** A noise density in V/√Hz has half exponents. `Dim` keeps whole ones and a `root` flag: the
  dimension is the square root of what the exponents describe (kg²·m⁴·s⁻⁵·A⁻² with `root`), shown with halves as
  `kg·m²·s⁻⁵ᐟ²·A⁻¹`. `Dim::sqrt` and `mul` / `div` keep it canonical: `root` only while some exponent is odd, so
  √(m²) is m; a root of a root isn't a `Dim` (`checked_sqrt` is `None`).

```rust
use ww_si::units::{Byte, Per, Sample};
use ww_si::Unit;

let per_sample = <Per<Byte, Sample> as Unit>::DIM;
assert_eq!(per_sample.to_string(), "bit·Sample⁻¹");
assert!(per_sample.mul(<Sample as Unit>::DIM).same(<Byte as Unit>::DIM));

let density = <ww_si::units::Volt as Unit>::DIM.div(<ww_si::units::Hertz as Unit>::DIM.sqrt());
assert_eq!(density.to_string(), "kg·m²·s⁻⁵ᐟ²·A⁻¹");
```

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
every method above; it has no products, and clients that don't know it show it by name. Its `UnitExpr` can be a
power too, `UnitExpr::Pow(&METER, 2)` is shown as `m²`; a marker whose `UnitExpr` is not a named one writes its
[run-time form](#units-known-only-at-run-time) `SI_EXPR` as well.

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
timestamps can be in different prefixes, and the timestamps can change prefix and storage from one to the next.

Without a full reading of the clock, `Unwrapper::default()` takes the first timestamp as is (the earliest time it can
be) and counts on from there; `restart()` starts over, for a device whose clock restarted. This is what
`wire_weaver_client` does for every time a device sends ([timestamps](../api/stream_reliability.md#timestamps)).

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

## In `ww`, Python and dynamic clients

A client that only has a device's introspection data still knows a quantity for what it is: `ww introspect` and
`ww observe` print `3300 mV`, Python gets a `ww.Quantity`. The unit comes from the marker: a marker of `ww_si::units`
by its name through `units::by_name` (symbol, dimension and scale), `Per<A, B>` and `Times<A, B>` as a tree of their
parts (`0.00012 N·m/(rad/s)`), and a marker of your own API crate (`struct Rpm;`) by its name and docs, without a
dimension or a conversion to SI. This works for ww_si 0.2 and later.

```python
q = dev.supply.read()        # Quantity(3300 mV U16)
str(q), float(q)             # "3300 mV", 3.3
q.symbol, q.dim              # "V", {"kg": 1, "m": 2, "s": -3, "A": -1}
q.to_si()                    # 3.3
dev.supply.write(1.5)        # 1.5 V
```

In Rust, `wire_weaver_client::quantity::DynQuantity::from_value(&value, &ty, &bundle)` gives the prefix, the number
and a `DynUnit`; its `Display` is the same text as the typed `Quantity`'s, `to_si_f64()` converts through the unit's
scale and `to_value()` makes a value to write. See `wire_weaver_py/README.md` for the Python side.

An [`AnyQuantity`](#units-known-only-at-run-time) goes the same way: `DynQuantity::from_value` reads its unit from the
value, a named unit of ww_si becomes the same `DynUnit` as the typed quantity's (so `3300 mV` either way), a device's
own unit keeps the dimension and scale it sent, and `ww`, Python and `to_value()` treat it like any other quantity.
`DynUnit` is shown and converted by `ww_si::tree`, the code behind the typed `Display`: a client with its own unit
model implements `ww_si::tree::Tree` for it and gets the same text.

## Units known only at run time

Some units are not known when the API is written: an ADC input whose unit is set by configuration, a generic sensor
channel, a calibration that turns volts into bar. For those the unit goes on the wire as an `SIExpr`, and a value
with one is an `AnyQuantity { prefix, value, unit }`:

```rust
use ww_si::units::{self, AngularVelocity, Per, Torque};
use ww_si::{AnyQuantity, Dim, Magnitude, Prefix, SIExpr, Scale, Volt};

// a ww_si unit, or a typed quantity turned into one
let v = AnyQuantity::from(Volt::milli(3300u16));
assert_eq!(v.to_string(), "3300 mV");
assert_eq!(format!("{v:#}"), "3.3 V");
assert_eq!(v.to_quantity::<units::Volt>(), Some(Volt::milli(3300u16)));
let unit = SIExpr::of::<Per<Torque, AngularVelocity>>();
assert_eq!(unit.to_string(), "N·m/(rad/s)");

// a device's own unit: its dimension and scale make it convertible to SI anyway
let rpm = SIExpr::Named {
    name: "Rpm",
    symbol: "rpm",
    dim: Dim::new(-1, 0, 0, 0, 0, 0, 0).with_rad(1),
    // exactly π/30 rad/s: 2π per 60 s
    scale: Scale::Exact(Magnitude::new(1, 30, 0, 0, 1)),
};
let speed = AnyQuantity::new(Prefix::Kilo, 3u8, rpm);
assert_eq!(speed.to_string(), "3 krpm");
```

`SIExpr` is the same model as the markers, put on the wire: `Named { name, symbol, dim, scale }`, `Mul`, `Div` and
`Pow` of other `SIExpr`s (through `RefBox`, so decoding one reads only its top node). A marker's `UnitExpr` maps one
to one onto an `SIExpr` (`Unit::SI_EXPR`, `SIExpr::of::<U>()`; `SIExprOwned::from(&UnitExpr)` on a host), and back
`SIExpr == UnitExpr` compares the trees and `known()` finds the ww_si unit of a named one. Symbol, name, dimension
and factor are computed by the same code for both, so a run-time unit is shown exactly like the typed one. A tree
that comes from another device is walked at most `MAX_UNIT_DEPTH` (32) levels deep: a deeper part has no dimension
and is shown as `…`, it can't overflow the stack.

`Dim` is nine 4-bit signed exponents in the order s, m, kg, A, K, mol, cd, rad, bit (each -8..=7), the `root` bit,
then four optional counted bases (`Counted { crate_name, name, exp }`), `final_structure`. `Dim::new` is `const` and refuses an exponent
out of range at compile time; `checked_mul`, `checked_div` and `checked_pow` give `None` for a run-time tree whose
exponents leave the range or that needs a fifth counted base. `Scale` is a nibble and then its numbers. These are the
bytes:

| Value                                        | Bytes                                         |
|----------------------------------------------|-----------------------------------------------|
| `Dim` of `Volt` (s⁻³ m² kg A⁻¹)              | `d2 1f 00 00 00 00`                           |
| `Dim` of `AngularVelocity` (s⁻¹ rad)         | `f0 00 00 01 00 00`                           |
| `Dim` of `Per<Byte, Sample>` (bit Sample⁻¹)  | `00 00 00 00 14`, `ww_si`, `Sample`, `f0 65`  |
| `Scale::Coherent`                            | `0`                                           |
| `Scale::Pow10(-3)`                           | `1` `fd`                                      |
| `Scale::Affine` of °C                        | `2` and two `f64`, 17 B                       |
| `Scale::Exact` of a degree (π/180)           | `30 fe 80 fe 80 02 00 00 83 fc 00 00`         |
| `SIExpr::of::<units::Volt>()`                | `00 56 6f 6c 74 56 d2 1f 00 00 00 00 00 01 14`, 15 B |
| `SIExpr::of::<Per<Torque, AngularVelocity>>()` | 53 B                                        |
| `AnyQuantity::from(Volt::milli(3300u16))`    | `12 e4 0c`, the 15 B of the unit, `79`: 19 B  |

The unit costs bytes on every value (19 bytes instead of 3 for 3300 mV), so a value whose unit is known when the
API is written stays a typed quantity, and a channel's unit is better sent once in its description than with every
reading: `ww_adc` has `ChannelInfo::unit: SIExpr<'i>` and plain numbers in its readings.

## Prefixes

`Prefix` covers every SI prefix from `Quecto` (10⁻³⁰) to `Quetta` (10³⁰), the binary ones `Kibi` to `Tebi` (2¹⁰ to
2⁴⁰), and three families of exact scales without a name, each with one small parameter: `Pow10` (a power of ten
without a prefix, 10⁻⁴), `Pow2` (a Q format's 2⁻¹⁵) and `FullScale` (a full-scale fraction, 1/65535 for 16 bits). So a
`Quantity` holds the values of the `Fixed` scales firmware uses most, exactly; a family never holds a named prefix's
scale (`Pow10(-3)` is refused, it is `Milli`), so equal scales are equal prefixes. Other exact scales (π/2¹⁵, 1/5) are
no `Prefix`: such a value is an [`AnyQuantity`](#units-known-only-at-run-time).

| Value | `{}` | `{:#}` | Bytes |
|---|---|---|---|
| `Volt::milli(3300u16)` | `3300 mV` | `3.3 V` | `12 e4 0c` |
| `Quantity::<units::Byte>::new(Prefix::Kibi, 64u32)` | `64 KiB` | `65536 B` | `93 30 40 00 00 00` |
| `Second` of 12345u32 at `Pow10(-4)` | `12345·10⁻⁴ s` | `1.2345 s` | `d3 fc 30 39 30 00 00` |
| `Second` of 32768u32 at `Pow2(-15)` | `32768·2⁻¹⁵ s` | `1 s` | `e3 f1 30 00 80 00 00` |
| `Ratio` of 32768u32 at `FullScale(16)` | `32768·1/65535` | `0.5000076295109483` | `f3 10 30 00 80 00 00` |
| `Ratio` of 5u32 at `Unknown(1000)` | `5·<scale 1000>` | | `8d f1 30 05 00 00 00` |

The most used prefixes (`Unit`, `Milli`, `Micro`, `Nano`, `Pico`, `Kilo`, `Mega`, `Giga`) take one nibble on the wire,
the other named ones a byte, and the three families one more byte for their parameter, so a scale takes 2 bytes at
most and ordinary values pay nothing for the others. In RAM a `Prefix` is four bytes, on a host and on M0, M4F and
M33 alike, and a `Volt` twelve (as before the unnamed scales came in).

A newer version may add kinds of scale; each is tag-only, its numbers packed into the `UNib32` tag. An older reader
gets it as `Prefix::Unknown(tag)` (a `#[future_version]` catch-all, see
[the derive reference](../serdes/derive_reference.md#no_future_version-future_version-name-enums-only)), skips the value and reads the rest of the message; nothing
converts, adds or compares such a value except to one of the same scale, and `{}` shows it as `·<scale N>`.

`pow10()` is the power of ten (`Some(-3)` for `Milli`, `Some(-4)` for `Pow10(-4)`, `None` for `Kibi` or 1/65535),
`magnitude()` the exact scale of any prefix but `Unknown` (`None` there), `Prefix::from_magnitude(&m)` the prefix of an
exact scale if there is one, `symbol()` the symbol of a named prefix (`m`, `µ`, `k`, `Ki`). Arithmetic across two
decimal prefixes stays on powers of ten; only a binary prefix or a family goes through `Magnitude`.

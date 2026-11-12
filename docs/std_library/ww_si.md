---
level: basic
---

# ww_si

`ww_si` gives device APIs typed physical values: a quantity is an SI [prefix](#prefixes) plus a `ww_numeric`
`NumericValue` in whatever number type the device picks, with the unit in the type. The API says "this is a
voltage", each device decides whether that is a `u16` of mV or an `f32` of V, and the host reads them all.

```rust
use ww_si::{Ampere, Overflow, Prefix, Rounding, Volt, Watt};

let v = Volt::milli(3300u16);          // prefix Milli, value U16(3300)
let w = Volt::from_f32(3.3);           // prefix Unit, value F32(3.3)
assert_eq!(Volt::milli(1500u16), Volt::base(1.5f32)); // compares values, not representations
assert_eq!(v.to_f64(), 3.3);           // in V
assert_eq!(w.to_in::<i32>(Prefix::Milli, Rounding::NearestAway, Overflow::Checked), Some(3300));
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

A marker knows its unit through the `Unit` trait: its dimension (`Dim`, exponents of s, m, kg, A, K, mol, cd and any
number of distinguished bases such as rad, bit and counted things, see [dimensions](#dimensions)), its name and symbol, and how a value becomes coherent SI (`Scale`: coherent, a power of ten, a factor, or a factor
and an offset for °C). `Volt::unit()` returns it; `ww_si::units::by_name("Volt")` finds it by the marker's name, which
is what a client that only sees introspection data uses.

## Quantity, Fixed or AnyQuantity

All three carry a value with its unit; they differ in what is fixed when the API is written and what travels with
every value:

| | Unit | Prefix and number type | On the wire | Pick it for |
|---|---|---|---|---|
| `Fixed<U, P, T, B>` | in the type | in the type | the number alone: `Micros<u32>` is 4 B | the API knows the scale and the storage: a timeout in ms as `u16`, a duty cycle in 1/65535, an ADC step |
| `Quantity<U, B>` | in the type | per value | a prefix and number type nibble, then the number: `Volt::milli(3300u16)` is 3 B | the API knows the unit, each device picks the scale and storage (`u16` of mV here, `f32` of V there) |
| `AnyQuantity` | per value | per value | the prefix, number and the unit: 3300 mV is 22 B with the unit by reference, 49 B in full | the unit is set at run time: configured sensor inputs, generic loggers |
| `Scaled<U, T>` | in the type | the scale per value, exact; the number type in the type | the number, then its `RunScale` in 9 B | the scale is read at run time but exact: a video stream's time base, a quantizer step ([below](#run-time-scales)) |

The units are one system in all three: the markers of `ww_si::units` (from [exact_units](exact_units.md)), with real
dimensions. A value only converts into another of the same dimension: `Byte`, `Sample` and `Ratio` are each a
dimension of their own (information, a counted thing, one) and never convert into each other, whatever their numbers.

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
| `fields`       | `ElectricFieldStrength`, `MagneticFieldStrength`, `CurrentDensity`, `ChargeDensity`, `Permittivity`, `Permeability`, `Resistivity`, `Conductivity`, .. |
| `mechanical`   | `Meter`, `Gram`, `Newton`, `Pascal`, `Joule`, `Torque`, `Litre`, `VolumetricFlow`, `Speed` .. `Yank` |
| `angle`        | `Radian`, `Degree`, `Steradian`, `AngularVelocity`, `AngularAcceleration` (rad is a dimension)    |
| `thermal`      | `Kelvin` (°C is a point, `origins::Celsius`)                                                      |
| `chemical`     | `Mole`, `Katal`                                                                                   |
| `photometric`  | `Candela`, `Lumen`, `Lux`                                                                         |
| `radiometric`  | `Irradiance`, `RadiantIntensity`, `Radiance`                                                      |
| `radiation`    | `Becquerel`, `Gray`, `Sievert`, `ElectronVolt`                                                    |
| `logarithmic`  | `DeciBel`, `DeciBelmW` (plain structs without a prefix)                                           |
| `dimensionless` | `Ratio`: duty, probability, scale (a group of its own, as it is used next to any other)          |
| `information`  | `Bit`, `Nibble` (4 bit), `Byte` (8 bit), `Halfword` (2 byte, 16 bit), `Word` (4 byte, 32 bit), `Doubleword` (8 byte, 64 bit) (bit is a dimension) |
| `counting`     | `Item`, `Sample`, `Frame`, `Packet`, `Line`, `Row`, `Record`, `Entry`, `Cycle`, `Block`, `Sector`, `Chunk`, `Char`, `Code`: counted bases |
| `quantities`   | all of the groups above                                                                           |
| `customary`    | imperial and US customary units (inch, pound, psi, ..), opt-in: not in `quantities` or `std`      |
| `nuclear`      | `Dalton`, `Barn`, `Curie`, `Rad`, `Rem`, `Roentgen`, named years, opt-in like `customary`         |
| `host`         | host helpers: [`Unwrapper`](#second-as-a-timestamp)                                               |
| `u16`, `f32`, .. `all` | number types of `NumericValue`, passed on to `ww_numeric` (see [ww_numeric](ww_numeric.md)) |

```toml
# firmware with voltages and timestamps, stored as u16, u32 and f32
ww_si = { version = "0.5", default-features = false, features = ["time", "electrical", "u16", "u32", "f32"] }
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
- `to_in::<T>(prefix, Rounding, Overflow)`: the value in units of `prefix` as the number type `T` (any primitive
  integer, `f32` / `f64` with those features). The call names what happens to a value that isn't a whole number of
  `prefix` (`Rounding`: `Truncate`, `Floor`, `Ceil`, `NearestEven`, `NearestAway`) and to one that doesn't fit `T`
  (`Overflow`: `Checked` gives `None`, `Saturate` the nearest end, `Wrap` modulo 2^bits, what a wrapping 32-bit
  counter of that unit shows, to compare a timestamp with a device clock). Exact for integer storage whatever the two
  scales are (SI, binary, `Pow10`, `Pow2`, `FullScale`); a float is first scaled to the float nearest the exact
  product, then rounded exactly. `Second::milli(5u16).to_in::<u32>(Prefix::Micro, Rounding::Truncate,
  Overflow::Checked)` is `Some(5000)`, `Volt::milli(3999u16).to_in::<u32>(Prefix::Unit, Rounding::NearestEven,
  Overflow::Checked)` is `Some(4)`;
- `to_si_f64()`: in coherent SI through the unit's scale: 90 ° is 1.5708 (rad), 20 °C is 293.15 (K), 5 g is 0.005 (kg);
- `rescaled(prefix, Rounding, Overflow)`: the same value in another prefix and the same number type, rounded and
  settled as `to_in` does, `None` also for a result outside the bounds `B`;
- `rescaled_exact(prefix)`: the same, only if that is exact (`Volt::milli(3000u16)` to `Unit` is 3 V; 3300 mV is
  `None`, and so is 3300 mV to µV, which doesn't fit a `U16`), for autoranging before sending;
- `same_repr(&other)`: same prefix and the same number in the same type, a cheap "did it change since I sent it".

`PartialEq` and `PartialOrd` compare values across prefixes and number types: `Volt::milli(1000u16) == Volt::base(1u8)`.
Two integers are compared exactly, even 60 orders of magnitude apart, and so are an `f32` and an integer (the float's
own value); two `f32`s at different prefixes compare by the scaled float, an `F64` (hosts) through `f64`.

On a small MCU the conversions, comparisons and bounds run on `exact_units`' integer kernel (`PrefixRatio`,
`Prefix::rescale`, `Prefix::cmp_scaled`) at the width of the enabled number types (`ww_numeric::Integer`, sign and
magnitude): with types up to 32 bits no `f32`, `f64` or 128-bit code is linked, and none for floats at all unless the
API turns on `f32` (`f64` is for hosts). The same prefix takes a shortcut on the stored numbers. The no-float guard
(`just no-float-guard`, part of `just test`) builds a firmware calling each of these operations for Cortex-M0 and
M4F and fails on any such symbol. Against the `i128` / `f64` paths this replaced, Donguru (Cortex-M0+, opt-level
`z`) is 14.6 KB smaller and rePort (Cortex-M33) 9.7 KB; a `FullScale` or binary-prefix conversion takes about a
tenth of the instructions it did, one between two SI prefixes about as many (`qemu-si-convert`).

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

A batched stream's generated `<name>_push_many(state, t0, dt, values)` stamps a block of samples the same way, on
the [time point](#which-time-point) of its `Timed` items.

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
- **Counted things.** `Item`, `Sample`, `Frame`, `Packet`, `Line`, `Row`, `Record`, `Entry`, `Cycle`, `Block`,
  `Sector`, `Chunk`, `Char` and `Code` count things: each is a distinguished base of its own, so `Per<Sample, Second>`
  (Sample·s⁻¹) isn't `Hertz` and samples aren't packets. Bytes of a block are `Per<Byte, Block>`.
- **Distinguished bases.** Radian, bit, neper and every counted thing are bases beside SI's seven, each named by the
  crate and the marker that declare it (`MarkerId`): two crates' `Sample` are different bases, and a marker moved to
  another module of its crate stays the same base. A dimension holds any number of them (exact_units UNIT-10). An API
  crate declares its own with a `unit!` line ending in `base` (`pub struct Bucket = "bucket", base;`), or a scaled
  unit of another one's base with `base(Other)` (the decibel is `base(Neper)`); `ww_si::base!(Bucket)` is its
  `MarkerId`. `rad` and `bit` are written by those names, any other base by its marker's name. On the wire the bases
  follow the exponents as a list sorted by crate and name; a list out of order, with a base twice or with an exponent
  of 0 is refused on decode, so every dimension has exactly one encoding. `Dim::strip(MarkerId::RADIAN)` drops a base
  on purpose (the joule of a torque, for v = ωr), `true_part()` drops all of them.
- **Square roots.** A noise density in V/√Hz has half exponents. Every exponent of a `Dim` counts halves (`Exp`,
  ±63½), so V/√Hz is kg·m²·s⁻⁵ᐟ²·A⁻¹. `DimOwned::checked_sqrt` refuses an odd count of halves: a fourth root isn't a
  dimension.

```rust
use ww_si::units::{Byte, Per, Sample, Sqrt};
use ww_si::Unit;

let per_sample = <Per<Byte, Sample> as Unit>::DIM;
assert_eq!(per_sample.to_string(), "bit·Sample⁻¹");
let bytes = per_sample.checked_mul(&<Sample as Unit>::DIM).unwrap();
assert!(bytes.same(<Byte as Unit>::DIM));

let density = <Per<ww_si::units::Volt, Sqrt<ww_si::units::Hertz>> as Unit>::DIM;
assert_eq!(density.to_string(), "kg·m²·s⁻⁵ᐟ²·A⁻¹");

let torque = <ww_si::units::Torque as Unit>::DIM;
assert_eq!(torque.strip(ww_si::MarkerId::RADIAN).to_string(), "kg·m²·s⁻²");
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

`Timed<V, Tm = DeviceMicros<u32>> { t: Tm, value: V }` is a value with the time it was taken: a sample of a
property, an item of a stream (`Timed<Ampere>`). `t` is a [point](points.md) on a time scale, by default
`DeviceMicros<u32>`: µs since the device booted (origin `DeviceBoot`) in a `u32` that wraps every 71.6 minutes. It is
a `final_structure` struct of `t` and then `value`, the same bytes a `#[derive_shrink_wrap(final_structure)]` struct
of those two fields has, so no fields can be added to it later. `t` is the bytes of its storage alone, no prefix and
no number tag. In an API it is used with its arguments, `property!(ro current: Timed<Ampere>)`; its definition is in
the `ww_si` snapshot, so devices leave it out of their introspection data (see
[generic types in an API](../types.md#generic-types-in-an-api)).

```rust
use ww_si::{Ampere, Timed, origins::DeviceMicros};

let sample = Timed::new(DeviceMicros::from_raw(1_000_000u32), Ampere::milli(-1200i32));
assert_eq!(sample.t.raw(), 1_000_000);
```

### Which time point

`Tm` is any `TimePoint`: a `Point<O, Fixed<Second, P, T>>` of a linear origin in seconds, in integer steps `P`
stored in an integer `T`, without bounds. The step and the storage are fixed by the type, per stream. Pick the one
that holds the longest silence between two items of a stream, at the device's resolution:

| `Tm`                                               | Wraps after | Longest gap the host bridges |
|----------------------------------------------------|-------------|------------------------------|
| `DeviceMillis<u16>`                                | 65.5 s      | 32.7 s                       |
| `DeviceMicros<u32>` (default)                      | 71.6 min    | 35.8 min                     |
| `Point<DeviceBoot, Fixed<Second, Pow2<-15>, u32>>` | 36.4 h      | 18.2 h                       |
| `DeviceMillis<u32>`                                | 49.7 days   | 24.8 days                    |
| `DeviceMicros<u64>`                                | never       | no unwrapping needed         |

A device that keeps a known time scale sends it the same way: `UnixMicros<u64>` for one that keeps UTC. A device
whose resolution changes at run time picks one `Tm` fine and wide enough for all of it; a fixed `u32` column in a
`Series` costs less than a per-item prefix and number tag did. A batched stream picks its `Tm` with
[`batched(time = ..)`](../api/batching.md).

On the device only the wrapping operations of `TimePoint` are used: `t.wrapping_add(dt)` and `t0.steps(dt)` (`t0`,
`t0 + dt`, `t0 + 2·dt`, .., wrapping like the clock), with `dt` a `TimeStep<Tm>` (`Fixed<Second, Micro, u32>` for
the default). `TimePoint::low(full)` takes the low bits of a full reading of the clock. `+` of a `u32` point panics
in a debug build once the clock wraps.

```rust
use ww_si::{TimePoint, TimeStep, origins::DeviceMicros};

let t0 = DeviceMicros::from_raw(4_294_967_000u32);
let dt = TimeStep::<DeviceMicros<u32>>::new(40);
// the times of 32 samples at 40 µs, the first one taken at t0, wrapping past 2^32 µs
let times: Vec<DeviceMicros<u32>> = t0.steps(dt).take(32).collect();
assert_eq!(times[8].raw(), 24);
```

### The full time on the host

`TimeUnwrapper<Tm>` gets the full point back, `TimePoint::Full` (`DeviceMicros<u64>` for the default): each point is
taken as the one closest to the previous with the same low bits. It is anchored on a full reading of the same clock
(`new(now)`, `anchor(now)`), or without one (`default()`) takes the first point as is and counts on from there;
`restart()` starts over for a device whose clock restarted. Use one per stream. The typed client does this for a
timed batched stream: its items carry `t: Option<DeviceMicros<u64>>`.

```rust
use ww_si::{TimeUnwrapper, origins::DeviceMicros};

let now = DeviceMicros::<u64>::from_raw(5_000_000_123); // the device's clock read in full
let mut unwrapper = TimeUnwrapper::<DeviceMicros<u32>>::new(now);
let full = unwrapper.unwrap(DeviceMicros::from_raw(705_032_827)).unwrap(); // u32 µs from a stream
assert_eq!(full, now);
```

A full `DeviceBoot` point is placed on another scale with `t.against(boot)`, `boot` being where the device booted on
that scale (a `UnixMicros<u64>`): exact, an offset only, right for a device whose clock was set or steered. A
device clock that runs at its own rate goes through a clock sync fit first.

In a `Series` (see [series](../serdes/series.md)) `t` is a column like any other: 32 `Timed<Volt>` samples at a
40 µs clock are 21 bytes.

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
parts (`0.00012 N·m/(rad/s)`), a unit of your own API crate (`unit!`) by the definition the model carries (INT-13,
its identity, symbol, dimension and exact scale), and a hand-written marker without one by its name and docs only.
This works for ww_si 0.2 and later.

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
value, a reference through the bundle (`ww_self`'s `ApiBundleOwned` is a `UnitTable` of every unit the API uses) and
then `ww_si::units::TABLE` (`quantity::units_of`). A named unit of ww_si becomes the same `DynUnit` as the typed
quantity's (so `3300 mV` either way), a device's own unit keeps its definition, and a reference no table lists is a
`DynUnit::Ref`, shown by its name. `ww`, Python and `to_value()` treat it like any other quantity; `to_value()`
compacts the unit against the bundle, so the API's units go out by reference. A capture (`ww tap`) stores the
device's bundle, so its values decode with no API crate at hand.
`DynUnit` is shown and converted by `ww_si::tree`, the code behind the typed `Display`: a client with its own unit
model implements `ww_si::tree::Tree` for it and gets the same text.

## Units known only at run time

Some units are not known when the API is written: an ADC input whose unit is set by configuration, a generic sensor
channel, a calibration that turns volts into bar. For those the unit goes on the wire as an `SIExpr`, and a value
with one is an `AnyQuantity { prefix, value, unit }`:

```rust
use ww_si::units::{self, AngularVelocity, Per, Radian, Second, Torque};
use ww_si::{AnyQuantity, Magnitude, MarkerId, Prefix, PrefixPolicy, Provenance, SIExpr, Unit, UnitDef, UnitNames, Volt};

// a ww_si unit, or a typed quantity turned into one: the unit's definition in full
let v = AnyQuantity::from(Volt::milli(3300u16));
assert_eq!(v.to_string(), "3300 mV");
assert_eq!(format!("{v:#}"), "3.3 V");
assert_eq!(v.to_quantity::<units::Volt>(), Some(Volt::milli(3300u16)));
let unit = SIExpr::of::<Per<Torque, AngularVelocity>>();
assert_eq!(unit.to_string(), "N·m/(rad/s)");

// the same value naming its unit by reference, for a reader that has a table of the units (the API's model)
let r = v.compact_in(&units::TABLE, &mut []).unwrap();
assert_eq!(r.unit, SIExpr::Ref(MarkerId { crate_name: "exact_units", name: "Volt" }));
assert_eq!(r.to_si_f64(&units::TABLE), Some(3.3));
assert_eq!(r.to_si_f64(&()), None); // a table that doesn't list it: no factor

// a unit no table lists, in full: its dimension and scale make it convertible to SI anyway
let rpm = SIExpr::Def(UnitDef {
    id: MarkerId { crate_name: "my_api", name: "Rpm" },
    names: UnitNames::new("rpm", PrefixPolicy::Decimal),
    dim: <Per<Radian, Second> as Unit>::DIM,
    quantity: None,
    // exactly π/30 rad/s: 2π per 60 s
    scale: Magnitude::new(1, 30, 0, 0, 1),
    provenance: Provenance::Defined,
});
let speed = AnyQuantity::new(Prefix::Kilo, 3u8, rpm);
assert_eq!(speed.to_string(), "3 krpm");
```

`SIExpr` is the same model as the markers, put on the wire. A leaf is a named unit: `Def(UnitDef)`, its whole
definition (identity, names, dimension, kind, exact scale, provenance), or `Ref(MarkerId)`, its identity alone, the
declaring crate and the marker's name. Above them are `Mul`, `Div`, `Pow` and `Sqrt` of other `SIExpr`s (through
`RefBox`, so decoding one reads only its top node). A marker's `UnitExpr` maps one to one onto an `SIExpr` with `Def`
leaves (`Unit::SI_EXPR`, `SIExpr::of::<U>()`; `SIExprOwned::from(&UnitExpr)` on a host), which reads alone anywhere.

A `Ref` reads through a `UnitTable`, which maps an identity to its definition: `units::TABLE` lists the units of
exact_units, a `UnitExpr` lists its own named units, a slice of tables chains them, and the API model a client has
lists every unit the API uses, the device's own and exact_units' alike. So the walks take the table:
`AnyQuantity::to_si_f64(&table)`, `display(&table)`, `SIExpr::dim`, `magnitude` and `same`. `Display` reads through
`units::TABLE`, and `to_quantity::<U>()` through `U`'s own definition. A `Ref` the table doesn't list is shown by its
name and has no dimension or factor, so a conversion over it refuses. Two crates' units of one name are two units:
`my_api::Rpm` and `fan_api::Rpm` are told apart by their crate.

`compact_in(&table, &mut arena)` (no allocation; a named unit needs no arena) and `AnyQuantityOwned::compact(&table)`
turn each `Def` the table lists into a `Ref`, refusing a `Def` whose identity the table lists with another
definition; `resolve` does the reverse. A value sent beside its API carries the compact form: `wire_weaver_client`
compacts what it writes into an `AnyQuantity` against the API's model, and a device can compact against the units
it uses. A tree that comes from another device is walked at most `MAX_UNIT_DEPTH` (32) levels deep: a deeper part has
no dimension and is shown as `…`, it can't overflow the stack.

`Dim` is on the wire as `DimWire`: the seven exponents of s, m, kg, A, K, mol, cd as a list of `INib32` halves, then
the distinguished bases as a sorted list of `BaseExp { base: MarkerId, exp: Exp }`, `final_structure`. `Dim::new` is
`const`; `DimOwned::checked_mul`, `checked_div`, `checked_pow` and `checked_sqrt` give `None` for a run-time result
whose exponents leave ±63½ or a root that isn't whole halves. A unit's names (`UnitNames`: symbol, ASCII form, long
names, aliases, UCUM code, prefixes, exact_units IO-9) travel with its definition. A unit's scale is an exact
`Magnitude`. These are the bytes (strings and their lengths are laid out as shrink_wrap does it, lengths at the end):

| Value                                                  | Bytes                                                  |
|--------------------------------------------------------|--------------------------------------------------------|
| `Dim` of `Volt` (s⁻³ m² kg A⁻¹)                        | `b1 81 43 00 00 07`                                    |
| `Dim` of `Per<Volt, Sqrt<Hertz>>` (s⁻⁵ᐟ² m² kg A⁻¹)    | `91 81 43 00 00 07`                                    |
| `Dim` of `AngularVelocity` (s⁻¹ rad)                   | `30 00 00 00`, `exact_units`, `Radian`, `46 39 17`, 24 B |
| `Dim` of `Per<Byte, Sample>` (bit Sample⁻¹)            | `00 00 00 00`, `exact_units`, `Bit`, `40`, `exact_units`, `Sample`, `30 63 93 39 27`, 41 B |
| `Magnitude` one                                        | `00 00`                                                |
| `Magnitude` of a degree (π/180)                        | `fe 80 fe 80 02 00 00 00 00 83 fc 00 00`               |
| `SIExpr::of::<units::Volt>()`, a `Def`                 | `00`, `exact_units`, `Volt`, `V`, `00`, `volt`, `volts`, `80`, `V`, `11 05 41 b1 81 43 00 00 00 00 00 10 70 a4 39 3d`, 45 B |
| the same compacted, a `Ref`                            | `10`, `exact_units`, `Volt`, `04 39`, 18 B             |
| `SIExpr::of::<Per<Torque, AngularVelocity>>()`         | 204 B; compacted, two `Ref`s: 52 B                     |
| `AnyQuantity::from(Volt::milli(3300u16))`              | `12 e4 0c`, the unit's 31 B, `7b`: 35 B                |
| the same compacted                                     | `12 e4 0c`, the unit's 18 B, `2a`: 22 B                |

The unit costs bytes on every value (22 bytes by reference, 35 in full, instead of 3 for 3300 mV), so a value whose
unit is known when the API is written stays a typed quantity, and a channel's unit is better sent once in its
description than with every reading: `ww_adc` has `ChannelInfo::unit: SIExpr<'i>` and plain numbers in its readings.

## Run-time scales

A `Fixed`'s scale is a type; some scales are only known once the device runs, yet exact: a video stream's time base
(H.264's `num_units_in_tick ÷ time_scale`, an MP4 timescale), a JPEG quantization step, an HRD rate of
2^(6 + bit_rate_scale) bit/s. `ww_si::Scaled<U, T>` (exact_units' NUM-42) is a number `T` at such a scale, the unit
`U` still in the type, and `Ticks<T>` is `Scaled<Second, T>`:

```rust
use ww_si::{RunScale, Ticks};

// an MP4 track's timescale of 15360, read from the file
let base = RunScale::ratio(1, 15_360).unwrap();
let pts = Ticks::<u64>::new(15_360 * 10 + 512, base);
assert_eq!(pts.to_string(), "154112·1/15360 s");
```

A `RunScale` is `num / den · 2^pow2` with `num` and `den` odd and coprime `u32`s and `pow2` within ±127: one form per
value, so `RunScale::ratio(2002, 120_000)` is `RunScale::ratio(1001, 60_000)`, `(1001, 1875, -5)`. On the wire it is
`num` and `den` as `u32` and `pow2` as `i8` (`final_structure`, 9 bytes); a form that isn't the canonical one doesn't
decode, so a value has one encoding. A `Scaled` is its number, then its `RunScale`: the `Ticks<u64>` above is 17
bytes (`stdlib.si.ticks` in the [conformance corpus](../serdes/conformance.md)).

In the API model both are the derived structs, the unit of a `Scaled` its first type argument. `ww`, Python and the
dynamic client read and write a `Scaled` as the struct `{ value, scale: { num, den, pow2 } }` for now; showing it as a
quantity is CLT-71, and they don't run the canonical-form check, which is Rust code the model can't carry (SW-20): a
non-canonical scale they send is refused by the device. A scale that lives in another field or register (a SunSpec
`*_SF`, a BLE format descriptor's exponent) is STD-77.

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

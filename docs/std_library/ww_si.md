---
level: basic
---

# ww_si

`ww_si` gives device APIs typed physical values: a quantity is an SI [prefix](#prefixes) plus a `ww_numeric`
`NumericValue` in whatever number type the device picks. The API says "this is a voltage", each device decides
whether that is a `u16` of mV or an `f32` of V, and the host reads them all.

```rust
use ww_si::{Prefix, Volt};

let v = Volt::millivolts(3300u16);     // Volt { prefix: Milli, value: U16(3300) }
let w = Volt::volts_f32(3.3);          // Volt { prefix: Unit,  value: F32(3.3) }
assert_eq!(v, w);                      // compares values, not representations
assert_eq!(v.to_f64(), 3.3);           // in V
assert_eq!(w.to_i128_in(Prefix::Milli), Some(3300));
```

`Volt::millivolts(3300u16)` is 3 bytes on the wire: the prefix and number type nibbles, then the `u16`.

## Features

Everything is a cargo feature, so a firmware compiles only what it uses. `std` (the default) enables all of them.

| Feature        | What                                                                                              |
|----------------|---------------------------------------------------------------------------------------------------|
| `time`         | `Second`, `Hertz`, `HertzPerSecond`, `Baud`, [`Timed<T>`](#timed-values)                          |
| `electrical`   | `Ampere`, `Volt`, `Ohm`, `Farad`, `Siemens`, `Henry`, `Coulomb`, `Watt`, `Weber`, `Tesla`         |
| `mechanical`   | `Meter`, `KiloGram`, `Newton`, `Pascal`, `Joule`, `Torque`, `Litre`, `VolumetricFlow`, `Speed` .. `Yank` |
| `angle`        | `Radian`, `Degree`, `Steradian`, `AngularVelocity`, `AngularAcceleration`                         |
| `thermal`      | `Kelvin`, `DegreeCelsius`                                                                         |
| `chemical`     | `Mole`, `Katal`                                                                                   |
| `photometric`  | `Candela`, `Lumen`, `Lux`                                                                         |
| `radiation`    | `Becquerel`, `Gray`, `Sievert`, `ElectronVolt`                                                    |
| `logarithmic`  | `DeciBel`, `DeciBelmW`                                                                            |
| `dimensionless` | `Ratio`: duty, probability, scale (a group of its own, as it is used next to any other)          |
| `quantities`   | all of the groups above                                                                           |
| `host`         | host helpers: [`Unwrapper`](#second-as-a-timestamp)                                               |
| `u16`, `f32`, .. `all` | number types of `NumericValue`, passed on to `ww_numeric` (see [ww_numeric](ww_numeric.md)) |

```toml
# firmware with voltages and timestamps, stored as u16, u32 and f32
ww_si = { version = "0.1", default-features = false, features = ["time", "electrical", "u16", "u32", "f32"] }
```

The bytes on the wire are the same for every feature set.

## Constructors and conversions

Every quantity has `new(prefix, value)`, taking any enabled number type (`u16`, `f32`, `UVlq32`, ..), and:

- `to_f64()` / `to_f32()`: the value in the base unit (V, A, Hz, s);
- `to_f64_in(prefix)`: in another prefix, `Volt::volts_f32(3.3).to_f64_in(Prefix::Milli)` is about 3300.0;
- `to_i128_in(prefix)`: as an integer, exact for integer storage (truncated toward zero when going to a bigger
  prefix), rounded for floats; `None` when it doesn't fit.

Named constructors exist for the quantities the IO traits use most:

| Type     | Constructors                                                                               |
|----------|--------------------------------------------------------------------------------------------|
| `Volt`   | `volts`, `millivolts`, `microvolts`, `kilovolts`, `volts_f32`                              |
| `Ampere` | `amperes`, `milliamperes`, `microamperes`, `nanoamperes`, `amperes_f32`                    |
| `Hertz`  | `hertz`, `millihertz`, `kilohertz`, `megahertz`, `gigahertz`, `hertz_f32`                  |
| `Second` | `seconds`, `milliseconds`, `microseconds`, `nanoseconds`, `picoseconds`, `seconds_f32`     |
| `Baud`   | `baud`, `kilobaud`, `megabaud`, `baud_f32`                                                 |
| `Torque` | `newton_meters`, `millinewton_meters`, `newton_meters_f32`                                 |
| `Ratio`  | `ratio`, `percent`, `per_mille`, `ppm`, `ratio_f32`                                        |

`PartialEq` and `PartialOrd` compare values across prefixes and number types: `Volt::millivolts(1000u16) ==
Volt::volts(1u8)`. Two integers are compared exactly, even 60 orders of magnitude apart; otherwise through `f64`.

The `quantity!` macro is a shorthand for literals: `quantity!(3300 mV u16)`, `quantity!(400 kHz u32)`, `quantity!(115200 Bd u32)`, `quantity!(250 mNm i32)`, `quantity!(95 % u8)`.

`Baud` is a symbol rate, the baud rate of a UART or a protocol decoder, kept apart from `Hertz` so a baud rate
is never taken for a clock frequency (and a tool shows `115200 Bd`, not `115.2 kHz`). `Torque` (N·m) has the
dimension of `Joule` but is kept apart the same way. `Ratio` is a plain fraction (0.5 is half; `percent` is
`Centi`), for a duty cycle, a probability or a scale factor; `DeciBel` is the logarithmic one.

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

let mut unwrapper = Unwrapper::new(Second::microseconds(5_000_000_123u64)).unwrap(); // Clock::now()
let full = unwrapper.unwrap(Second::microseconds(705_032_827u32)).unwrap();        // U32 µs from a stream
assert_eq!(full, Second::microseconds(5_000_000_123u64));
```

That works while consecutive timestamps are less than half a wrap period apart; re-anchor with `anchor(now)` after
a longer pause. Use one `Unwrapper` per stream. `U64` and signed values pass through unchanged. The anchor and the
timestamps can be in different prefixes.

## Timed values

`Timed<T> { t: Second, value: T }` is a value with the time it was taken, `t` a device timestamp as above: a
sample of a property, an item of a stream (`Timed<Ampere>`). It is a `final_structure` struct of `t` and then
`value`, the same bytes a `#[derive_shrink_wrap(final_structure)]` struct of those two fields has, so no fields can
be added to it later.

```rust
use ww_si::{Ampere, Second, Timed};

let sample = Timed::new(Second::microseconds(1_000_000u32), Ampere::milliamperes(-1200i32));
```

## Prefixes

`Prefix` covers every SI prefix from `Quecto` (10⁻³⁰) to `Quetta` (10³⁰), `From<Prefix> for i8` gives the power
of ten. The most used ones (`Unit`, `Milli`, `Micro`, `Nano`, `Pico`, `Kilo`, `Mega`, `Giga`) take one nibble on
the wire.

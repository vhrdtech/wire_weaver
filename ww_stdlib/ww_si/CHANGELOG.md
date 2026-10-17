## Unreleased

### ⚠️ Breaking

ww_si 0.2 (STD-38): every quantity is one generic type. The bytes on the wire are the same as in 0.1 (values,
`Series` and `Series<Timed<..>>`), the API model and Rust API are not: devices and hosts are rebuilt with 0.2 together.

* Every quantity is `Quantity<U: Unit> { pub prefix, pub value, _unit: PhantomData<U> }`, and every unit a zero-size
  marker in `ww_si::units` (`units::Volt`). The 0.1 names are aliases (`pub type Volt = Quantity<units::Volt>`), so
  APIs keep writing `Volt`. Struct literals don't compile (`_unit` is private): `Volt { prefix, value }` becomes
  `Volt::from_parts(prefix, value)` (`const`) or a prefix constructor; a pattern needs `..`. In introspection a use of
  `Volt` is `Instance(Quantity, [units::Volt])`.
* `KiloGram` is now `Gram` (unit g, scale 10⁻³ kg): 5 kg is `Gram::kilo(5u8)`, `quantity!(5 g u32)` was
  `KiloGram { Milli, .. }`.
* The named constructors are gone, the generic prefix constructors replace them on every quantity: `volts`,
  `seconds`, `hertz`, .. → `base`; `millivolts`, `milliseconds`, .. → `milli`; `micro*` → `micro`, `nano*` → `nano`,
  `pico*` → `pico`, `kilo*` → `kilo`, `mega*` → `mega`, `gigahertz` → `giga`; `volts_f32`, `seconds_f32`, .. →
  `from_f32`. `Ratio` keeps `percent`, `per_mille` and `ppm` (`ratio` → `base`, `ratio_f32` → `from_f32`).
  `Second::to_f64_seconds()` → `to_f64()`.
* `quantity!` is removed: `quantity!(3300 mV u16)` is `Volt::milli(3300u16)`.
* The `Meter`-per-`Second` conversion (`TryFrom<(Meter, Per, Second)> for Speed`, `Per`, `ToF32`, `Error`) is removed:
  `Meter / Second` is a `Speed` (an `f32` one). `ww_si::units::Per` is now the compound unit marker.
* Removed the unfinished dynamic types `Unit<'i>`, `Quantity<'i>` and the `SIExpr` stub; `Unit` is now the unit
  marker trait. `BaseUnit`, `SIExp` and `INib8P3` stay as they were, for `ww_adc::ChannelInfo::unit`, until STD-37
  replaces them with a run-time unit model.
* `DeciBel` and `DeciBelmW` stay plain structs without a prefix.

### 🚀 Features

* `Dim` (SI exponents of s, m, kg, A, K, mol, cd, `Display` as `kg·m²·s⁻³·A⁻¹`), `Scale` (coherent, power of ten,
  factor, affine for °C), `UnitExpr` (a named unit or a product / quotient tree, with `dim()`, `factor()`,
  `symbol()`, `name()`) and the `Unit` trait (`DIM`, `EXPR`): an API crate can declare its own marker.
* Written once for every quantity: `from_parts` (`const`), `new`, `base`, `milli`, `micro`, `nano`, `pico`, `centi`,
  `kilo`, `mega`, `giga`, `from_f32` (`const`); `quantize(x, prefix, storage)` (an `f32` rounded and saturated into
  the representation an API documents, computed in `f32`); `to_f64`, `to_f32`, `to_f64_in`, `to_i128_in`;
  `to_si_f64` (coherent SI through the unit's scale: 90 ° → 1.5708 rad, 20 °C → 293.15 K); `with_prefix` (exact
  re-prefix in the same number type); `same_repr`; `unit()`; `PartialEq` / `PartialOrd` across prefixes and number
  types as before.
* `Display` (`3300 mV`, `{:#}` in the unit itself: `3.3 V`; a ratio as `50 %`, `‰`, `ppm`), `Debug`
  (`Volt(3300 mV U16)`) and `defmt::Format` (`3300 mV`) for every quantity; `Prefix::symbol()`.
* Unit algebra checked by the compiler: `UnitMul` / `UnitDiv` from a product table (`Volt * Ampere = Watt`,
  `Torque * AngularVelocity = Watt`, `Newton * Meter = Joule`, `Gram * Acceleration = Newton`, .. 15 lines, each
  giving `A·B`, `B·A`, `C/A`, `C/B`), every line checked against the SI exponents at compile time. `*` / `/` between
  quantities, `+` / `-` within one unit, `* f32` / `/ f32`, and any unit times or divided by a `Ratio` keeps its unit;
  computed in `f32` (`f64` with an `F64` operand) through each unit's scale, stored as `F32` / `F64` without a prefix
  (feature `f32`). `Volt * AngularVelocity` and `Volt + Ampere` don't compile.
* Compound units `units::Per<A, B>` and `units::Times<A, B>`, shown as `N·m/(rad/s)`.
* `units::by_name("Volt")` and `units::ALL`: the `UnitExpr` of every named unit, for dynamic clients.

### 🐛 Fixes

* `to_f32()` scales by one exact power of ten instead of repeated `* 10` / `/ 10`, so `Second::milli(2u8)` is
  `0.002` rounded once.

## 0.1.0

Not published on crates.io; its definitions are in the API snapshot `api_snapshots/ww_si_0_1_0.ron`.

### ⚠️ Breaking

* Number types are cargo features, passed on to `ww_numeric` (`f32`, `u16`, .., `all`); `std` enables all of them.
  A `no_std` build names the ones it uses: `ww_si = { .., default-features = false, features = ["f32"] }`.
  Conversions (`Speed` from `Meter` per `Second`) handle the enabled types only.
* Quantities are cargo features by group (STD-23): `time` (`Second`, `Hertz`, `HertzPerSecond`), `electrical`
  (`Ampere`, `Volt`, `Ohm`, `Farad`, `Siemens`, `Henry`, `Coulomb`, `Watt`, `Weber`, `Tesla`), `mechanical` (`Meter`,
  `KiloGram`, `Newton`, `Pascal`, `Joule`, `Litre`, `VolumetricFlow`, `Speed` .. `Yank`), `angle`, `thermal`,
  `chemical`, `photometric`, `radiation`, `logarithmic`, `quantities` for all of them. `std` enables all. A `no_std`
  build names the groups it uses: `features = ["electrical", "f32"]`. `Speed` from `Meter` per `Second` needs
  `mechanical` and `time`.

### 🚀 Features

* `Timed<T> { t: Second, value: T }` (feature `time`): a value with the time it was taken, `t` a device timestamp.
  `final_structure`, derived with a type parameter (DER-14) and `series`, so `Series<Timed<T>>` works. Usable in
  APIs (API-43, `property!(ro current: Timed<Ampere>)`): its generic definition is in the 0.1.0 snapshot, so devices
  leave it out of their introspection data.
* `Ratio` (new feature group `dimensionless`, in `quantities`): a duty cycle, probability or scale factor, with
  `ratio`, `percent`, `per_mille`, `ppm`, `ratio_f32` and `quantity!(95 % u8)`.
* `Torque` (feature `mechanical`): N·m, kept apart from `Joule`, with `newton_meters`, `millinewton_meters`,
  `newton_meters_f32`, `quantity!(250 mNm i32)` and `quantity!(2 Nm u8)`.
* `Prefix` and every quantity (`Ratio` and `Torque` too) can be a field of a `shrink_wrap::series::Series` item (SW-60, `series` directive): a
  quantity is a run of its prefix and a numeric column of its value that restarts where the prefix changes. See
  `tests/series.rs` for the design's example, 14 bytes for 4 `Volt` / `Ampere` samples.
* Constructors and conversions on every quantity (STD-23): `Volt::new(Prefix::Milli, 3300u16)` (any number type
  through `From<..> for NumericValue`), `to_f64()` / `to_f32()` in the base unit, `to_f64_in(prefix)`,
  `to_i128_in(prefix)` (exact for integers). Named constructors for `Volt` (`volts`, `millivolts`, `microvolts`,
  `kilovolts`, `volts_f32`), `Ampere` (`amperes` .. `nanoamperes`, `amperes_f32`), `Hertz` (`hertz`, `millihertz`,
  `kilohertz`, `megahertz`, `gigahertz`, `hertz_f32`) and `Second` (`seconds` .. `picoseconds`, `seconds_f32`,
  `to_f64_seconds()`).
* `PartialEq` and `PartialOrd` across prefixes and number types: `Volt::millivolts(1000u16) == Volt::volts(1u8)`,
  exact for integers (no overflow even 60 orders of magnitude apart), through `f64` for floats.
* `Second` as a device timestamp is documented: time since the device's clock started, fixed-width storage wraps
  (STD-23). `Unwrapper` (feature `host`, in `std`) turns wrapped timestamps back into the full time, anchored on a
  full-width reading of the clock (`ww_timebase::Clock::now()`).
* Number features `uvlq32` and `uvlq64` (in `all`), for `NumericValue::UVlq32` / `UVlq64`.
* `Baud` (feature `time`): a symbol rate, the baud rate of a UART or a decoder, with `baud`, `kilobaud`,
  `megabaud`, `baud_f32` and `quantity!(115200 Bd u32)`; the IO traits use it instead of a bare `u32` of baud.
* `quantity!` takes `ms`, `us`, `ns`, `Hz`, `kHz`, `MHz` and `uA`.
* Every quantity type, `Prefix`, `BaseUnit`, `SIExp` and `INib8P3` derive `Clone` and `Debug`, have an owned
  variant with the `std` feature and `defmt::Format` with the `defmt` feature (only `Volt` and `Second` had them),
  so they can be used in API types (STD-13).
* `From<Prefix> for i8` (the power of ten) instead of `Into<i8> for Prefix`, the `Into` direction still works.

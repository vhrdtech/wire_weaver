## Unreleased

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
* `quantity!(5 g u32)` builds a `KiloGram` in `Prefix::Milli` (it named a `Gram` type that doesn't exist).

### 🚀 Features

* `Timed<T> { t: Second, value: T }` (feature `time`): a value with the time it was taken, `t` a device timestamp.
  `final_structure`, written by hand with the bytes the derive gives the same two fields (the derive has no type
  parameters), so it is not in the API snapshot yet.
* `Ratio` (new feature group `dimensionless`, in `quantities`): a duty cycle, probability or scale factor, with
  `ratio`, `percent`, `per_mille`, `ppm`, `ratio_f32` and `quantity!(95 % u8)`.
* `Torque` (feature `mechanical`): N·m, kept apart from `Joule`, with `newton_meters`, `millinewton_meters`,
  `newton_meters_f32`, `quantity!(250 mNm i32)` and `quantity!(2 Nm u8)`.
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

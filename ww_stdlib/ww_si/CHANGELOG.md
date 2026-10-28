## Unreleased

### 🚀 Features

* No 64- or 128-bit code on small targets (STD-43): comparisons (`PartialEq` / `PartialOrd` of `Quantity`, `DeciBel`,
  `DeciBelmW`), `with_prefix`, the exact arithmetic (`checked_add`, `checked_sub`, `checked_mul_int`, `wrapping_add`,
  `wrapping_sub`, `steps`), `Display` and `defmt::Format` work on `ww_numeric::Integer` instead of widening every value
  to `i128`. The same prefix compares the stored numbers; across prefixes the value with the bigger prefix is scaled by
  powers of ten in `u32` with an overflow check, an overflow means it is the bigger one; signs and `u32` against `i32`
  go by sign and magnitude. Results are the same and exact. The `f64` fallback is only reached when one value is a
  float, so a firmware without float types has no `f64` code from it either. Donguru (STM32G0B1, thumbv6m, number types
  `u16` / `u32` / `i32`, `PartialEq` of settings and observe values), flash `.text`: 286 448 → 280 292 B (−6 156 B) at
  `opt-level = "z"`, where `scale_i128` (1 720 B), `i128::checked_pow` (1 278 B), `NumericValue::to_i128`, `__muldf3`
  and `__divdf3` are gone and `ops::compare` with its helpers is 316 B; 405 144 → 401 924 B (−3 220 B) at
  `opt-level = 3`, `ops::compare` 4 006 → 760 B, none of them calling `__aeabi_lmul` or another 64-bit helper (`llvm-nm` /
  `llvm-objdump`).
* `Quantity::to_i32_in(prefix)` and `to_u32_in(prefix)`: the value in units of `prefix` as `i32` / `u32`, exact for
  integers (truncated toward zero to a bigger prefix), rounded for floats (in `f32` for an `F32`), `None` when it
  doesn't fit; `u32` arithmetic only. `Quantity::to_u32_wrapping_in(prefix)`: the same modulo 2^32, to compare a
  timestamp with a wrapping 32-bit device clock.
* `Quantity`, `Prefix`, `Timed` and the other `series` types derive `shrink_wrap::series::SeriesCells` (API-50,
  observe cells), so quantities can be properties of an observe cells level.

### ⚠️ Breaking

* Against earlier builds from main (EVO-16): `Dim` on the wire is the 41-bit form with rad, bit, a root flag and
  counted bases (was seven exponents in 28 bits, `sized`), so `SIExpr`, `AnyQuantity` and `ww_adc::ChannelInfo::unit` change bytes;
  devices and hosts built from main before this are rebuilt together. `Dim` has a lifetime (`Dim<'i>`, a constant
  `Unit::DIM: Dim<'static>`), `tree::Tree<'a>` too. `Scale` has a new variant `Exact` (an `ExactScale`), which
  `Degree` now uses (was a factor), so its `SIExpr` bytes change too. `Torque`, the angle units, `Lumen` and `Lux` have rad in their
  dimension, `Byte` is 8 `Bit` (a scale, the dimension is bit).
* Against earlier builds from main (STD-61): the compile-time core is the new crate `exact_units`, which `ww_si`
  re-exports (`ww_si::exact_units`, and every moved name at the same place as before: `ww_si::Dim`, `ww_si::units`,
  `ww_si::Fixed`, ..), so most code compiles unchanged. Moved: `Dim`, `Counted`, `Scale`, `ExactScale`, `UnitExpr`,
  the `Unit` trait, `UnitMul` / `UnitDiv`, the unit markers, `Prefix`, `SIExpr` / `SIExprOwned`, `tree` (without
  `quantity`), `Fixed` with its scales, storage and bounds. `Fixed::to_quantity` / `Fixed::try_from_quantity` are
  `Quantity::from_fixed` (or `Quantity::from(fixed)`) / `Quantity::to_fixed` (a crate can't add inherent methods to a
  type of another). The counted markers of this crate (`Item`, `Sample`, ..) are now known by the crate
  `exact_units`, so a `Dim` with one is 6 bytes longer on the wire and isn't the same base as one named by `ww_si`.
  In introspection the moved types are `exact_units::..`, which `wire_weaver_client` reads as units.

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
  marker trait. `BaseUnit`, `SIExp` and `INib8P3` (whose serdes were `todo!()`) are removed too (STD-37): a unit
  known only at run time is an `SIExpr`, a value with one an `AnyQuantity` (below). `BaseUnit::Volt` becomes
  `SIExpr::of::<units::Volt>()`, `BaseUnit::Named { name, symbol, exp }` becomes
  `SIExpr::Named { name, symbol, dim, scale }`.
* `DeciBel` and `DeciBelmW` stay plain structs without a prefix.
* `Quantity::to_i128_in` exists only with a 64- or 128-bit number type enabled (`u64`, `i64`, `uvlq64`, `u128`, `i128`,
  or `std`). With types up to 32 bits use `to_i32_in` / `to_u32_in` (STD-43).
* `Quantity::checked_mul_int(n)` takes `impl Into<ww_numeric::Integer>` instead of an `i64`: literals and `i32` / `u32`
  / smaller integers work as before, an `i64` only with a 64-bit number type enabled. `Steps` keeps its step as an
  `Integer`.
* `wrapping_add`, `wrapping_sub` and `steps` give `None` when `rhs` in `self`'s prefix is beyond the widest enabled
  integer type (`u32::MAX` with types up to 32 bits); `Steps` no longer ends when a value plus the step would leave
  `i128`, it wraps in its number type as every other step does.

### 🚀 Features

* `Unwrapper::default()` (no anchor: the first timestamp is taken as is, the earliest time it can be) and
  `Unwrapper::restart()`, for a host without a full reading of the device's clock; timestamps may change prefix and
  storage from one to the next (P2401#17oct-2214b). `Unwrapper::last()` returns `Option<Second>` (`None` before the
  first timestamp without anchor).
* Exact integer arithmetic on quantities of the same unit (P2401#17oct-2214), next to the float operators:
  `checked_add` / `checked_sub` (in the finer prefix of the two and `self`'s number type, `Second::milli(5u32)` plus
  `Second::micro(40u16)` is `Second::micro(5040u32)`), `checked_mul_int(n)`, `wrapping_add` / `wrapping_sub` (in
  `self`'s prefix and number type, wrapping like a device clock) and `steps(&dt)`, the iterator `t0`, `t0 + dt`, ..
  (`Steps<U>`). `None` on overflow, for floats, and where a step is not a whole number of the prefix.
* `Prefix::ALL`, `Prefix::name()` (`"Milli"`) `Prefix::from_name()` (the variant names a dynamic client sees in a
  decoded value) and `Prefix::scale_f64` / `scale_f32` (a number in units of the prefix to the unit itself), for
  dynamic clients (CLT-31).
* `Dim<'i>` (exponents of s, m, kg, A, K, mol, cd, rad, bit and up to four counted bases, `Display` as
  `kg·m²·s⁻³·A⁻¹`), `Scale` (coherent, power of ten,
  factor, affine for °C), `UnitExpr` (a named unit or a product / quotient / power tree, with `dim()`, `factor()`,
  `symbol()`, `name()`) and the `Unit` trait (`DIM`, `EXPR`, `SI_EXPR`): an API crate can declare its own marker.
* Units known only at run time (STD-37), the same model as the markers on the wire:
  * `Dim` is nine `i8` exponents (-8..=7, an `I4` each on the wire), `root: bool` and `counted: [Option<Counted>; 4]`, `final_structure` (Volt
    is `d2 1f 00 00 00 00`, 41 bits): `Dim::new(s, m, kg, a, k, mol, cd)` (`const`, a compile error out of range),
    `checked_new`, `exps()`, `checked_mul` / `checked_div` / `checked_pow` for run-time trees (`None` out of range
    or for a fifth counted base). `Scale` goes on the wire too (a nibble, then its numbers).
  * `SIExpr<'i>` { `Named { name, symbol, dim, scale }`, `Mul`, `Div`, `Pow` } with `RefBox` children, and
    `SIExprOwned`: a unit as a tree, for a configurable sensor channel (`ww_adc::ChannelInfo::unit`). `dim()`,
    `factor()`, `offset()`, `symbol()` / `Display`, `name()` give what `UnitExpr` gives, written by the same code, so a
    run-time unit is shown exactly like the typed one (`N·m/(rad/s)`, `Per<Torque, AngularVelocity>`). Every walk
    stops at `MAX_UNIT_DEPTH` (32) levels: a tree from another device can't overflow the stack.
  * `SIExpr` ⇄ `UnitExpr`: `Unit::SI_EXPR` (`Per` / `Times` build their tree in `const`), `SIExpr::of::<U>()`,
    `SIExpr::named(expr)` (`const`), `SIExprOwned::from(&UnitExpr)` for any tree; back, `SIExpr == UnitExpr` (same
    tree) and `known()` (the `ww_si` unit of a named one).
  * `AnyQuantity<'i> { prefix, value, unit: SIExpr }` and `AnyQuantityOwned`: `From<Quantity<U>>`,
    `to_quantity::<U>()`, `to_f64`, `to_si_f64`, `Display` like a typed quantity (`3300 mV`, `{:#}` `3.3 V`), `Debug`,
    `defmt::Format`. `AnyQuantity::from(Volt::milli(3300u16))` is 19 bytes, the typed value's 3 and the unit's 16.
  * `UnitExpr::Pow(expr, n)`, shown as `m²`, `(m/s)²`, named `Pow<Meter, 2>`.
  * `ww_si::tree`: `Tree` and `Node` (a named node's dimension and scale can be `None`, for a unit known by name only)
    with `symbol`, `name`, `dim`, `factor`, `offset`, `same` and `quantity` (a quantity's `Display`): the one walk
    behind `UnitExpr`, `SIExpr` and the typed `Display`, for a client's own unit model (`wire_weaver_client::quantity`).
* Angle, information and counted things are dimensions (STD-60), so bytes never turn into % or rad and a rate of
  samples isn't a frequency:
  * rad is a base: `Radian` rad, `AngularVelocity` rad·s⁻¹ (not `Hertz`), `AngularAcceleration` rad·s⁻²,
    `Steradian` rad², `Lumen` cd·rad², `Lux` cd·rad²·m⁻², `Torque` J/rad (kg·m²·s⁻²·rad⁻¹, still shown `N·m`, so
    `Torque * AngularVelocity = Watt` and a torque isn't a joule). `Dim::strip_radian()` is the explicit bridge to
    SI's rad = 1, for exports and v = ωr.
  * bit is a base, feature `information`: `Bit` (coherent), `Byte` (8 bit), `Word` (32 bit).
  * counted bases, feature `counting`: `Item`, `Sample`, `Frame`, `Packet`, `Cycle`, `Block`, `Sector`, `Chunk`,
    `Char`, `Code`, each a dimension of its own known by its crate and marker name (`Counted<'i> { crate_name, name,
    exp }`: two crates' `Sample` are different bases, a marker moved between modules stays the same; shown by the
    name). Up to four in one `Dim` (`MAX_COUNTED`), kept sorted by crate and name; a `Dim` whose bases are out of
    order, repeated or with a zero exponent is refused on decode. `Per<Byte, Sample>` times `Sample` is a `Byte`.
    `ww_si::counted!(Name)` is the dimension of an API crate's own counted marker, with its crate filled in.
  * square roots (`root: bool`): V/√Hz is kg²·m⁴·s⁻⁵·A⁻² with `root`, shown `kg·m²·s⁻⁵ᐟ²·A⁻¹`; `Dim::sqrt` /
    `checked_sqrt` (`None` for a root of a root), `mul` / `div` / `checked_pow` square the plain side and halve a
    result whose exponents are all even, so `root` is set only with an odd exponent (else refused on decode).
  * `Dim::ONE`, `Dim::counted(crate_name, name)`, `with_rad`, `with_bit`, `counted_bases()`, `is_one`, `same`, `mul` /
    `div` (`const`, a compile error out of range), `is_canonical`; `DimOwned` / `CountedOwned` with `as_dim()`.
* `Fixed<U, P, T, B = Unbounded>` (STD-55): a number whose unit, scale and storage are fixed by the API, on the wire
  exactly the number (`transparent`, `#[repr(transparent)]`), so `shared_us: u64` becomes
  `shared: Fixed<Second, Micro, u64>` with the same bytes and introspection shows `µs`. No prefix and no
  `NumericValue` on the device: a `u64` here doesn't turn on `ww_numeric`'s `u64` for the firmware.
  * Storage (`Storage`, `UnsignedStorage`): `u8`..`u64`, `i8`..`i64`, `f32`, `f64`, `UNib32`, `UVlq32`, `UVlq64`,
    `Nibble`.
  * Scale markers in `ww_si::prefix` (trait `FixedScale`, `DecimalPrefix`, `SubSecond`): `Quecto`..`Quetta`, `One`,
    binary `Kibi`, `Mebi`, `Gibi`, `Tebi`, and factors without a name `Pow10<E>` (tenths of a millisecond:
    `Fixed<Second, Pow10<-4>, u32>`) and `Rational<NUM, DEN>`.
  * Bounds (`Bounds<T>`, `OutOfBounds`): `Unbounded`, `Range<MIN, MAX>`, `RangeU64`, `RangeDec<MIN, MAX, EXP>`
    (decimal bounds for floats); checked on every read from the wire (`validate`), NaN is out of bounds, a bound that
    doesn't fit the storage is a compile error.
  * `new` / `get`, the operators `+ - * /` by `T` (and the assigning ones, `Sum`) for unbounded values; `try_new`,
    `bounded` (`const`, a compile error out of bounds), `unbounded`, `to_bounds` for bounded ones; `checked_*`,
    `saturating_*`, `wrapping_*`; `to::<P2, T2>()` (exact conversion only, `None` otherwise: 1500 µs isn't a whole
    number of ms), `Quantity::from_fixed` / `From<Fixed> for Quantity` (only for number types `NumericValue` has
    enabled), `Quantity::to_fixed`;
    `core::time::Duration` both ways for s, ds, cs, ms, µs, ns; `PartialEq`, `Ord`, `Hash`; `Display` (`1500 µs`,
    `{:#}` in the unit: `0.0015 s`, a scale without a prefix as `15·10⁻⁴ s`), `Debug`, `defmt::Format`;
    `series(cells)`, so a `Series` of `Fixed` fields has the columns of the plain fields.
  * Aliases `Micros<T>`, `Millis<T>`, `Nanos<T>`, `Bytes<T>`, `Ppb<T>`, `Duty16` (`Fixed<Ratio, Rational<1, 65535>,
    u16>`, 65535 is 100 %).
* `ExactScale { num, den, pow10, pow2, pi }`: a factor written with integers only (`10⁻⁶`, `2¹⁰`, `1/65535`, `π/180`,
  `2π`), always canonical (`num` / `den` coprime and free of 2 and 5), so every reader computes the same value and two
  scales compare field by field; `new`, `try_new`, `pow10`, `pow2`, `rational`, `mul`, `div`, `recip`, `apply_int`,
  `apply_f64`, `to_f64`, `as_pow10`, `Display` (`·10⁻⁴`, `·π/180`). A non-canonical one from the wire is refused.
  `Scale::Exact(ExactScale)` is the same on a unit known at run time: `Degree` is exactly `π/180` rad, and the new unit
  `Turn` (`rev`, alias `ww_si::Turn`) is `2π` rad.
* `units::Sqrt<U>`: the square root of a unit (V/√Hz is `Per<Volt, Sqrt<Hertz>>`), a `Dim` with `root`, shown `√Hz`,
  named `Sqrt<Hertz>`; `UnitExpr::Sqrt` and `SIExpr::Sqrt` (a new last variant, part of the EVO-16 break) carry it
  at run time. `exact_units::num::sqrt_f64` is a `const` square root, rounded exactly like `f64::sqrt`.
* `to_hertz` / `from_hertz` on `Quantity<Per<Cycle, Second>>` and `to_hertz` on a `Fixed` of it: cycles per second
  become hertz only through this named step.
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

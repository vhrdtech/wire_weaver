## Unreleased

### ⚠️ Breaking

* `UnitExpr::Named` has a `provenance: Provenance<'static>` field (STD-72): a `UnitExpr::Named { .. }` written by
  hand adds `provenance: Provenance::Defined`; a pattern without `..` adds `..`.
- `Prefix` (STD-59) gains the binary prefixes `Kibi`, `Mebi`, `Gibi`, `Tebi`, three families of exact scales without a name, each with a one-byte parameter, and a catch-all: `Pow10(Pow10Exp)` (`10⁻⁴`; `-63..=63`, not an SI prefix's exponent), `Pow2(Pow2Exp)` (`2⁻¹⁵`, a Q format or an RTC tick; not a binary prefix's), `FullScale(FullScaleBits)` (`1/(2^n − 1)`, `1/65535` for a 16-bit duty) and `#[future_version] Unknown(u16)` (DER-25), a scale kind from a newer version, read past with the rest of the message intact. No table, in flash or on the wire: a family's scale is computed from its parameter, and equal scales are equal prefixes (a family refuses a named prefix's scale, also when read). Other exact scales (`1/5`, a BAM's `π/2¹⁵`) are no `Prefix`: such a value is a `ww_si::AnyQuantity`. On the wire named prefixes keep their one nibble (the others a byte), a family takes one byte more; scale kinds added later are tag-only, their numbers packed into the `UNib32` tag, so an older reader gets `Unknown(tag)`. In memory `Prefix` is 4 bytes on host, M0, M4F and M33 (`niche`, DER-24; 8 with a plain `#[repr(u32)]`), the size it had, so `ww_si::Volt` stays 12 bytes. A device or host built before this refuses a value in a binary prefix or a family (decode error); every other value has the same bytes. One break for all of it.
  - `From<Prefix> for i8` and `TryFrom<i8> for Prefix` are removed (no answer for a binary prefix): use `pow10() -> Option<i8>` (also `Some` for `Pow10`), the const `si_pow10()` (SI prefixes only, the fast path), `si_from_pow10(e)` (const, SI only), `from_pow10(e)` (SI, else `Pow10`) or `from_pow2(e)`, and `magnitude() -> Option<Magnitude>` for the exact scale (`None` for `Unknown`).
  - New: `Pow10Exp`, `Pow2Exp`, `FullScaleBits` (`new` refuses a named prefix's scale, `get`; each with `magnitude`), `Prefix::from_magnitude` (the prefix of an exact scale, if one stands for it) / `named_of` / `is_named` / `param` / `name` / `from_name`, `Prefix::ALL` has the binary ones. `Display` writes `Ki`, a family as `·10⁻⁴`, `·2⁻¹⁵`, `·1/65535`, and `Unknown` as `·<scale N>`; `scale_f64` / `scale_f32` are exact for every prefix (`NaN` for `Unknown`).
  - A `Series` store and observe cells keep a `Prefix` as one code (hand-written `SeriesStore` / `SeriesCells`; rows of one named prefix are written column-wise, as before); an `Unknown` comes back from the store as `Unknown(u16::MAX)`, its tag isn't kept.
  - `Prefix::MAX_SIZE` is 20 bits (was 8): `Series::<ww_si::Volt>::max_bytes(100)` is 2254 B (was 1946 B), a buffer sized by it grows that much. The bytes of a named prefix don't change.

### 🚀 Features

- `Magnitude`, `MagnitudeRest`, `Irrational` and `PrimePower` are `self_describing` instead of `final_structure` (STD-59): the same bytes (every field ends by itself; the `stdlib.si.magnitude.*` conformance cases unchanged), and a magnitude can now be the field of a `self_describing` enum's variant. Never released as `final_structure`.
* Feature `serde` (INT-13): `DimOwned`, `CountedOwned` and `Magnitude` are `serde::Serialize` / `Deserialize`, for API snapshots in RON (`ww_self::UnitDef`). A `Magnitude` is written as its parts, exponents doubled (`(pow10: -6)`, `(pow10: 0, pi: 2, primes: [(3, -2)])`), and read back through `Magnitude::from_parts_x2`, so a file can't hold a non-canonical one.
* `Offset::checked_add`, `None` where `add` panics (offsets read from an API model); `Offset` is `serde::Serialize` / `Deserialize` with feature `serde` (a negative zero is refused).
* `Magnitude::from_parts_x2(pow10_x2, pow2_x2, Irrational, primes)`: the magnitude of the parts `pow10_pow2_x2`, `irrational_x2` and `primes` give, an error (new `MagnitudeError::NotPrime`) for a number that isn't a prime other than 2 and 5.
* `Provenance` (STD-72): whether a named unit's scale is its definition (`Defined`) or a measured value a source
  prints (`Conventional { source, year }`). `unit!` takes `conventional("NIST SP 811", 2008)` after the scale; the
  14 measured customary units (inHg and inH2O at a temperature, Btu and calorie at a temperature or mean, boiler and
  water horsepower) carry it. With feature `shrink_wrap` it is an evolvable wire enum (`Provenance<'i>`, owned
  `ProvenanceOwned`, `Unknown(_)` for a kind of source a newer writer adds) and `Default` (`Defined`), so an API model
  carries it (`ww_self::UnitDef::provenance`).
* `units` imperial, US customary and other units outside the SI (STD-72, feature `customary`, part of
  `quantities`): 166 markers, each the exact rational of SI its legal definition gives (1959 yard and pound, NIST SP
  811 and Handbook 44, UK Weights and Measures Act 1985, g_n), the source cited in its docs; `units::CUSTOMARY` lists
  them and `units::by_name` finds them. No ambiguous bare name: `GallonUs` / `GallonImperial`, `OunceAvdp` /
  `OunceTroy` / `FluidOunceUs`, `TonShort` / `TonLong` / `Tonne`, `BtuIt` / `BtuTh` / `Btu39f`.., `CalorieIt` /
  `CalorieTh` / `CalorieFoodIt`, `HorsepowerMechanical` / `HorsepowerMetric` / `HorsepowerElectric` /
  `HorsepowerBoiler`, `Thou` vs `MilNato` / `MilWarsawPact`, `FootInternational` / `FootUsSurvey`,
  `InchOfMercuryConventional` / `InchOfMercury32f`; `PoundAvdp` (mass), `PoundForce`, `Slug` (mass), `Poundal`,
  `Kip`, `Psi`, `FootPoundForce`.
* `Fixed::convert::<U2, P2, T2>()` (STD-72): the same value in another unit of the same dimension, exactly or `None`
  (`lbf·s` to `pN·s`, `in` to `mm`), never reinterpreted; the factor `U · P / (U2 · P2)` worked out at compile time
  like `Fixed::to`. Units of different dimensions or with an offset don't compile.
  `UnitExpr::magnitude()`, `Scale::magnitude()` and `tree::magnitude()`: the exact factor to SI of a unit (tree),
  `None` with a `Factor` or `Affine` scale.
* `unit!` and `scale!` (INT-13): unit and scale markers declared as data, one line each (`pub struct Rpm = "rpm",
  [rad 1, s -1], Exact(1 / 30, pi 1);`, `pub struct Lsb70 = "", Exact(70 / 1000);`), expanding to the marker struct
  and its `Unit` / `FixedScale` (and `DecimalPrefix`) impl. WireWeaver's walker reads the same lines, so an API's own
  markers are known to its clients. Every marker of `units` and the named prefixes of `prefix` are declared this way;
  their impls are the same consts as before. A hand-written `impl Unit` still compiles.
* `units::ElectronVolt`, `Byte` and `Word` have an exact scale (`Scale::Exact`, eV = 1602176634·10⁻²⁸ exactly since
  SI 2019) instead of an `f64` `Scale::Factor`; `unit!` has no `Factor(f)` form: an exact factor is a `Magnitude`, one
  known only at run time is a value, not a unit.
* `Scale::Factor(f64)` is gone: every factor of a unit is exact (`Scale::Exact`), and a free `f64` factor would make
  readers compute different values. On the wire `Affine` is now nibble `2` and `Exact` `3` (were `3` and `4`): a
  `SIExpr` with °C from a device built with `ww_si` before this reads wrong (no published release had it).

* `Point<O, F>` and origins (STD-64): a position against an origin, its number a `Fixed` (`transparent`, on the wire
  exactly the number). Point ± vector = point, point − point = vector (`PointDiff`, signed for an origin that wraps),
  point + point and point · x don't compile; `checked_add`, `checked_sub`, `checked_diff`, `try_from_raw`, `Display`
  in the origin's own scale (`98.6 °F`), `Debug`, `defmt::Format`.
  * `Origin` (unit, `Reference`, `Modulus`, `OFFSET`, `STEP`, `SHOWN`, `DEF: OriginDef`) and `origin!`, declaring one
    from the unit's zero (`{ unit:, symbol:, offset: [, step:][, reference:][, modulus:] }`) or above another
    (`{ above: Celsius, offset: Offset::int(25) }`, a register formula).
  * `Offset`: a signed exact `Magnitude` (`Offset::int`, `ratio`, `decimal`), canonical, `final_structure` on the wire,
    a negative zero refused on decode.
  * `origins`: `AbsoluteZero`, `Celsius`, `Rankine`, `Fahrenheit`, `Vacuum`, `Gauge`, `AboveStandardAtmosphere`,
    `Angle<Turn>`, `Angle<Degree>`, `Angle<Radian>`; references `UnitZero`, `AmbientPressure`; moduli `Linear`,
    `Modulo<S>`.
  * `to::<O2, P2, T2>()` converts between origins of the same unit and reference with factor and offset worked out at
    compile time, exact or `None`; gauge to absolute doesn't compile, `against(zero)` takes the reference at run time.
  * Wrapping: a period of 2^BITS steps (`Bam16`) in the storage's wrapping arithmetic, any other by `rem_euclid`; a
    period that isn't a whole number of steps doesn't compile.
* Named rounding and overflow policies (STD-63, docs `std_library/rounding.md`): `Rounding` (`Truncate`, `Floor`,
  `Ceil`, `NearestEven`, `NearestAway`) and `Overflow` (`Checked`, `Saturate`, `Wrap`); `Fixed::to_rounded`,
  `to_saturating`, `to_wrapping` and `requantize` (both chosen at run time), exact, with π / ln factors through
  `Magnitude::fixed_point` (64 bits); `to` stays exact or `None`. `into_rounded` and `From<Fixed<.., Range<..>>>` for
  a whole factor are infallible, proven by the bounds at compile time (`Bounds::INT_RANGE`). Bounds markers
  `Wrapping` and `Saturating` set what `+ - * /` do; `Q15`, `Q31`, `UQ16_16` saturate, `Bam16`, `Bam32` wrap (made
  with `Fixed::of`, another policy with `with_overflow`). `requantize` / `Requantizer` with the factor and the
  storage (`StorageKind`) known at run time give the typed result bit for bit (conformance `requantize.*`).
  On Cortex-M M0 and M4F a typed conversion costs what the same conversion written by hand costs (measured, table
  on the docs page).

* `Index<Of, T, B>`, `IndexFrom1<Of, T, B>` and `Seq<Of, T>` (STD-58, feature `counting`, docs
  `std_library/indices.md`): positions in a table of `Of` and wrapping sequence numbers, the point side of counted
  things, `transparent` (the bytes of `T`).
  * `Index`: 0-based; index ± `Fixed<Of, One, T>` = index, index − index = the distance as a `Fixed`, index + index
    and index · x don't compile; `try_new`, `at`, `checked_add`, `checked_sub`, `checked_distance` for bounded ones,
    decode refuses a number outside `B`.
  * `IndexFrom1`: 1-based as people count; 0 and a number outside `B` refused on construction and decode;
    `to_index`, `from_index`.
  * `Seq`: `next`, `wrapping_add`, `wrapping_sub`, ± an amount, `distance` (a `Fixed<Of, One, T, Wrapping>`), RFC 1982
    `is_before`, `is_after`, `serial_cmp` within half the range; no `Ord`, no `Seq + Seq`.
  * `Display` is the number, `Debug` names the table (`Index<Sample>(3 u32)`), `defmt::Format`.

First release (STD-61): the compile-time core of `ww_si`, as a crate of its own, so code that only needs units in its
types doesn't depend on `ww_si`'s run-time quantities and `ww_numeric`. `ww_si` re-exports everything here at the
same paths as before.

No dependencies without features: the wire format is the feature `shrink_wrap` (on by default), every derive sits
behind `cfg_attr(feature = "shrink_wrap", ..)`, and the types are plain Rust otherwise (`Dim`'s exponents `i8`,
`Magnitude`'s primes `u32`, written as `I4` / `UNib32` through shrink_wrap's `#[wire(..)]`, DER-19). A
firmware that only needs units in its types builds without it; the bytes don't depend on the feature.

* `Fixed<U, P, T, B = Unbounded>` (STD-55): a number whose unit, scale and storage are fixed at compile time, on the
  wire exactly the number (`transparent`, `#[repr(transparent)]`).
  * Storage (`Storage`, `UnsignedStorage`): `u8`..`u64`, `i8`..`i64`, `f32`, `f64`, `UNib32`, `UVlq32`, `UVlq64`,
    `Nibble`.
  * Scale markers in `prefix` (trait `FixedScale`, `DecimalPrefix`, `SubSecond`): `Quecto`..`Quetta`, `One`, binary
    `Kibi`, `Mebi`, `Gibi`, `Tebi`, `Pow10<E>` and `Rational<NUM, DEN>`.
  * Bounds (`Bounds<T>`, `OutOfBounds`): `Unbounded`, `Range<MIN, MAX>`, `RangeU64`, `RangeDec<MIN, MAX, EXP>`;
    checked on every read from the wire, NaN is out of bounds, a bound that doesn't fit the storage is a compile
    error.
  * `new` / `get`, `+ - * /` by `T`, `try_new`, `bounded` (`const`), `checked_*`, `saturating_*`, `wrapping_*`,
    `to::<P2, T2>()` (exact only), `from_scaled(Repr, Magnitude)`, `core::time::Duration` both ways, `PartialEq`,
    `Ord`, `Hash`, `Display` (`1500 µs`, `{:#}` `0.0015 s`), `Debug`, `defmt::Format`, series columns of the plain
    field.
  * Aliases `Micros<T>`, `Millis<T>`, `Nanos<T>`, `Bytes<T>`, `Ppb<T>`, `Duty16`.
* Unit markers in `units` with `Unit` (`DIM`, `EXPR`, `SI_EXPR`), `UnitMul` / `UnitDiv` from a product table, `Per`,
  `Times`, `Pow`, and `Sqrt<U>` (new: V/√Hz is `Per<Volt, Sqrt<Hertz>>`, shown `√Hz`, named `Sqrt<Hertz>`).
* `Dim<'i>` (STD-60): exponents (`i8`, -8..=7) of s, m, kg, A, K, mol, cd, rad and bit, a `root` flag and up to four counted bases
  (`Counted { crate_name, name, exp }`); `counted!(Name)` for an API crate's own counted marker. The counted markers
  here (`Item`, `Sample`, `Frame`, `Packet`, `Cycle`, `Block`, `Sector`, `Chunk`, `Char`, `Code`) are known by the
  crate `exact_units`, 6 bytes longer on the wire than under the name `ww_si`.
* `Scale` (coherent, power of ten, factor, affine, `Exact`) and `Magnitude` (STD-62): one canonical exact factor for
  every scale, `10^a · 2^b · π^c · ln2^d · ln10^e · ∏ pᵢ^kᵢ` over primes other than 2 and 5 (`PrimePower`, at most
  `MAX_FACTORS` = 12), whole or half exponents (`√2`), so equal magnitudes are equal however spelled (`Milli`,
  `Pow10<-3>`, `Rational<1, 1000>`). `const` `ratio(u64, u64)` (factored at compile time: trial division, Miller-Rabin,
  Pollard's rho), `pow10`, `pow2`, `new(num, den, pow10, pow2, pi)`, `PI`, `LN2`, `LN10`, `mul`, `div`, `recip`, `powi`,
  `sqrt` and their `checked_` forms (`MagnitudeError`: `Zero`, `PrimeTooLarge` above `u32::MAX`, `TooManyFactors`,
  `ExponentRange` outside ±63½, `NotSquare`), `to_ratio`, `to_f64`, `as_pow10`, `exp_x2`, `irrational_x2`, `primes`,
  `apply_int`, `apply_f64`, `Display` (`·π/180`, `·√2`, `·ln10/20`). 104 bytes, plain `const` data, so a `&'static
  Magnitude` can be held at run time. On the wire (`final_structure`, `Irrational` and `MagnitudeRest` inside) a
  decimal prefix is 2 bytes, `Kibi` 5, a degree 11; a magnitude that isn't canonical is refused on decode. Conformance
  cases `stdlib.si.magnitude.*`.
* Scale markers `Pow2<E>`, `FullScale<V, BITS>` (`V / 2^BITS`, an ADC step), `FullScaleMax<V, BITS>` (`V / (2^BITS -
  1)`); `Rational<NUM, DEN>` takes `u64`. Aliases `Q15<U = Ratio>`, `Q31<U>`, `UQ16_16<U>`, `Bam16`, `Bam32` (STD-62).
* `Fixed::to::<P2, T2>()` works out `P / P2` at compile time (STD-62): between integer storages a factor that isn't a
  fraction of integers, or whose multiply doesn't fit 64 bits (both storages up to 32 bits) or 128 bits, is a
  compile error; a factor 1/n divides in the storage's own width, equal scales cost nothing. `Storage` has `BITS`,
  `MIN_INT`, `MAX_INT`, `to_i64`, `from_i64` and `div_whole` for it. On thumbv6m and thumbv7em at `opt-level` 3 the
  conversions measured (ms ↔ µs, equal scales, `Duty16`, `Q15`, a `Rational<70, 1000>` gyro scale) are the code of
  the same conversion written by hand, except ms → µs in `u32` (a range check instead of `umull`, +4 instructions on
  M4F); at `z` within ±3 instructions (table in `docs/std_library/magnitude.md`).
* `SIExpr<'i>` / `SIExprOwned` (feature `shrink_wrap`: `Named`, `Mul`, `Div`, `Pow`, and the new `Sqrt`), `UnitExpr::Sqrt`, `tree` (the walk
  behind every unit `Display` and name), `Prefix`.
* `num`: `const` helpers for scales, `pow10_f64`, `powi_f64`, `scale_f64`, `scale_f32`, and `sqrt_f64`, a `const`
  square root rounded exactly like `f64::sqrt`.

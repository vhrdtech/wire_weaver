## Unreleased

### 🚀 Features

First release (STD-61): the compile-time core of `ww_si`, as a crate of its own, so code that only needs units in its
types doesn't depend on `ww_si`'s run-time quantities and `ww_numeric`. `ww_si` re-exports everything here at the
same paths as before.

No dependencies without features: the wire format is the feature `shrink_wrap` (on by default), every derive sits
behind `cfg_attr(feature = "shrink_wrap", ..)`, and the types are plain Rust otherwise (`Dim`'s exponents `i8`,
`ExactScale`'s `num` / `den` `u32`, written as `I4` / `UNib32` through shrink_wrap's `#[wire(..)]`, DER-19). A
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
    `to::<P2, T2>()` (exact only), `from_scaled(Repr, ExactScale)`, `core::time::Duration` both ways, `PartialEq`,
    `Ord`, `Hash`, `Display` (`1500 µs`, `{:#}` `0.0015 s`), `Debug`, `defmt::Format`, series columns of the plain
    field.
  * Aliases `Micros<T>`, `Millis<T>`, `Nanos<T>`, `Bytes<T>`, `Ppb<T>`, `Duty16`.
* Unit markers in `units` with `Unit` (`DIM`, `EXPR`, `SI_EXPR`), `UnitMul` / `UnitDiv` from a product table, `Per`,
  `Times`, `Pow`, and `Sqrt<U>` (new: V/√Hz is `Per<Volt, Sqrt<Hertz>>`, shown `√Hz`, named `Sqrt<Hertz>`).
* `Dim<'i>` (STD-60): exponents (`i8`, -8..=7) of s, m, kg, A, K, mol, cd, rad and bit, a `root` flag and up to four counted bases
  (`Counted { crate_name, name, exp }`); `counted!(Name)` for an API crate's own counted marker. The counted markers
  here (`Item`, `Sample`, `Frame`, `Packet`, `Cycle`, `Block`, `Sector`, `Chunk`, `Char`, `Code`) are known by the
  crate `exact_units`, 6 bytes longer on the wire than under the name `ww_si`.
* `Scale` (coherent, power of ten, factor, affine, `Exact`) and `ExactScale { num, den, pow10, pow2, pi }`.
* `SIExpr<'i>` / `SIExprOwned` (feature `shrink_wrap`: `Named`, `Mul`, `Div`, `Pow`, and the new `Sqrt`), `UnitExpr::Sqrt`, `tree` (the walk
  behind every unit `Display` and name), `Prefix`.
* `num`: `const` helpers for scales, `pow10_f64`, `powi_f64`, `scale_f64`, `scale_f32`, and `sqrt_f64`, a `const`
  square root rounded exactly like `f64::sqrt`.

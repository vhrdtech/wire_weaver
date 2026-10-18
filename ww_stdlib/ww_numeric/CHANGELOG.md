## Unreleased

### 🚀 Features

* `SeriesStore::set` of `NumericValue` is `#[inline(always)]`: a known variant folds to two stores at `opt-level = "z"` too.
* `NumericValueStore` is built in place by `SeriesStore::init_store` (API-39).
* `NumericValueCells`, the `shrink_wrap::series::SeriesCells` of `NumericValue` (API-50, observe cells): the
  variant byte and the payload word of `NumericValueStore` as atomics, copied out column by column; `put` is
  `#[inline(always)]` (a known variant folds to two stores).

### ⚠️ Breaking

Wire-incompatible with 0.1.1.

* `NumericValue` variants are behind cargo features, one per number type (`nibble`, `u16`, `u32`, `unib32`, `u64`,
  `i32`, `f32`, `un`, `in`, `u128`, `i8`, `i16`, `i64`, `i128`, `f64`; `U8` is always there), `all` for every one.
  `std` enables `all`, so hosts see no change; a `no_std` build has to name the types it uses
  (`features = ["f32"]`). The value takes 8 bytes of RAM with types up to 32 bits instead of 24 to 32, and unused
  number types bring no code (1.5 KB less flash in the `mcu_qemu` GPIO example). The wire format is the same for
  every feature set; a variant that is not enabled is read as `shrink_wrap::Error::EnumVariantDisabled`.
* `NumericValue` is `#[non_exhaustive]`: a `match` outside this crate needs a `_` arm.
* `NumericValue::UNib32` holds a `UNib32` instead of a `u32` and is variable-length on the wire, as its name and
  `NumericBaseType::UNib32` say: `UNib32(5)` was `40 05 00 00 00` (a plain little-endian `u32` after the discriminant),
  now it is `45`; `300` is `4C D4` instead of `40 2C 01 00 00`. `ww_self` already read and wrote it this way, so values
  it decoded from such a field were wrong. Construct with `NumericValue::UNib32(UNib32(5))` or `UNib32(5).into()`.

* `NumericBaseType`: `U4` renamed to `Nibble`, variants reordered, `UN` and `IN` added.
* `NumericValue`: `U4` renamed to `Nibble`, variants reordered.

### 🚀 Features

* `NumericValueStore` writes rows with one variant column by column (SW-64, `SeriesStore::columnar` /
  `write_store`): the tag run and the variant's column straight from the payload array, the same bytes as the item
  walk. Not for `UN` / `IN`.
* `NumericBaseType::from_i128_wrapping(x)`: `x` wrapped at an integer type's width, for `ww_si`'s wrapping time
  arithmetic.
* `NumericValueStore<N>` (SW-62), the `shrink_wrap::series::SeriesStore` of `NumericValue`, written by hand: a byte
  for the variant and one payload array as wide as the widest variant enabled (`U8`, `U32`, `I32`, `F32`: 5 bytes
  per value where `NumericValue` takes 8), plus a byte for `UN` / `IN` widths. `NumericValue` is `series(no_store)`.
* `NumericBaseType::from_i128(x)` (exact, `None` when it doesn't fit) and `from_f32_saturating(x)` (rounded and
  saturated for integers): a value in a given number type, for `ww_si::Quantity::quantize` and `with_prefix`.
* `NumericValue` can be a field of a `shrink_wrap::series::Series` item (SW-60, `series` directive): a tag run of
  the storage type, then one column per type in use; a variant whose feature is off is `EnumVariantDisabled` on
  reading, as in plain form.
* `NumericValue::UVlq32(UVlq32)` and `UVlq64(UVlq64)` (features `uvlq32`, `uvlq64`, part of `all`), discriminants
  16 and 17, and `NumericBaseType::UVlq32` / `UVlq64` (STD-23): variable-length storage, 1 byte for values up to 127.
  `ww_numeric::{UVlq32, UVlq64}` re-exported, `value!(300 uvlq32)`.
* `From<u8>`, `From<u16>`, .., `From<f64>`, `From<UVlq32>` for `NumericValue` (each behind its type's feature),
  `NumericValue::to_f64()`, `to_i128()` (exact integer value), `unsigned_bits()` (where a counter wraps),
  `is_float()`.
* `Owned` variants and optional `serde` support.
* `NumericValue::ty()`, `NumericAnyTypeOwned::human_name()` and `default()`, `NumericBaseType::name()` (std).

### 📚 Documentation

- Docs: product names written WireWeaver and ShrinkWrap in prose.

## [0.1.1] - 2026-01-07

### ⚙️ Miscellaneous Tasks

* Bump shrink_wrap version to 0.1.2

## [0.1.0] - 2026-01-07

### 🚀 Features

* NumericBaseType
* NumericAnyType
* SubTypeKind
* NumericValue

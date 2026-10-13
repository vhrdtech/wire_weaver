## Unreleased

### ⚠️ Breaking

Wire-incompatible with 0.1.1.

* `NumericValue` variants are behind cargo features, one per number type (`nibble`, `u16`, `u32`, `unib32`, `u64`,
  `i32`, `f32`, `un`, `in`, `u128`, `i8`, `i16`, `i64`, `i128`, `f64`; `U8` is always there), `all` for every one.
  `std` enables `all`, so hosts see no change; a `no_std` build has to name the types it uses
  (`features = ["f32"]`). The value takes 8 bytes of RAM with types up to 32 bits instead of 24 to 32, and unused
  number types bring no code (1.5 KB less flash in the `mcu_qemu` GPIO example). The wire format is the same for
  every feature set; a variant that is not enabled is read as `shrink_wrap::Error::EnumVariantDisabled`.
* `NumericValue` is `#[non_exhaustive]`: a `match` outside this crate needs a `_` arm.

* `NumericBaseType`: `U4` renamed to `Nibble`, variants reordered, `UN` and `IN` added.
* `NumericValue`: `U4` renamed to `Nibble`, variants reordered.

### 🚀 Features

* `Owned` variants and optional `serde` support.
* `NumericValue::ty()`, `NumericAnyTypeOwned::human_name()` and `default()`, `NumericBaseType::name()` (std).

## [0.1.1] - 2026-01-07

### ⚙️ Miscellaneous Tasks

* Bump shrink_wrap version to 0.1.2

## [0.1.0] - 2026-01-07

### 🚀 Features

* NumericBaseType
* NumericAnyType
* SubTypeKind
* NumericValue

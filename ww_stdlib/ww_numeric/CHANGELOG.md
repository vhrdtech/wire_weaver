## Unreleased

### ⚠️ Breaking

Wire-incompatible with 0.1.1.

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

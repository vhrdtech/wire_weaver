## Unreleased

### ⚠️ Breaking

* Number types are cargo features, passed on to `ww_numeric` (`f32`, `u16`, .., `all`); `std` enables all of them.
  A `no_std` build names the ones it uses: `ww_si = { .., default-features = false, features = ["f32"] }`.
  Conversions (`Speed` from `Meter` per `Second`) handle the enabled types only.

### 🚀 Features

* `From<Prefix> for i8` (the power of ten) instead of `Into<i8> for Prefix`, the `Into` direction still works.

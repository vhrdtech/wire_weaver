## Unreleased

### ⚠️ Breaking

* Number types are cargo features, passed on to `ww_numeric` (`f32`, `u16`, .., `all`); `std` enables all of them.
  A `no_std` build names the ones it uses: `ww_si = { .., default-features = false, features = ["f32"] }`.
  Conversions (`Speed` from `Meter` per `Second`) handle the enabled types only.

### 🚀 Features

* `Second` derives `PartialEq` (for `ww_client_server::stream::StreamHead`).
* Every quantity type, `Prefix`, `BaseUnit`, `SIExp` and `INib8P3` derive `Clone` and `Debug`, have an owned
  variant with the `std` feature and `defmt::Format` with the `defmt` feature (only `Volt` and `Second` had them),
  so they can be used in API types (STD-13).
* `From<Prefix> for i8` (the power of ten) instead of `Into<i8> for Prefix`, the `Into` direction still works.

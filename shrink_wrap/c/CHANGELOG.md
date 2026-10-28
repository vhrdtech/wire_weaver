## Unreleased

### 🚀 Features

- `sw_requantize.h` / `sw_requantize.c` (CLT-54, STD-63): `sw_requantize()` converts a stored number to another scale and storage with a named rounding (`SW_ROUND_TRUNCATE`, `_FLOOR`, `_CEIL`, `_NEAREST_EVEN`, `_NEAREST_AWAY`) and overflow policy (`SW_OVERFLOW_CHECKED`, `_SATURATE`, `_WRAP`), the same number as Rust's `exact_units::Requantizer` and the typed `Fixed::requantize`, bit for bit. The factor comes as constants worked out on the host (`sw_factor`: an exact ratio of up to 128 bits, or the fixed point `m * 2^shift` for π and other factors that aren't fractions of integers); integers are exact in 128 bits without `__int128`, float storages under `#ifndef SW_NO_F64`. The `shrink_wrap_c` crate gets `requantize::CFactor::of` (the constants of a `Magnitude`) and `c_initializer`, and cross-checks C against Rust (`tests/requantize.rs`); the conformance corpus's requantize cases run in C.

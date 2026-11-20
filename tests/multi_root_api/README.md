# multi_root_api

API of two wire lines sharing a crate (EVO-37): `wire_v1::Thermo` at w1.2 and `wire_v2::Thermo` at w2.1, their
wire versions in `Cargo.toml`'s `[package.metadata.wire_weaver.roots]`, and the types and sub-trait both lines
share, unchanged, in `common`.

Used by `tests/multi_root`, never built on its own.

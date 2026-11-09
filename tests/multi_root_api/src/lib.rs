//! Two wire lines of one API in one crate (EVO-37): `wire_v1::Thermo` at w1.2 and `wire_v2::Thermo` at w2.1, their
//! wire versions in `Cargo.toml`'s `[package.metadata.wire_weaver.roots]`, and the types and the sub-trait both
//! lines share, unchanged, in `common`. Used by `tests/multi_root`.
#![no_std]

pub mod common;
pub mod wire_v1;
pub mod wire_v2;

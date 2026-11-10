//! The API of the multi-line bench (EVO-28, EVO-42), a small temperature logger, with every wire line still in use in
//! one crate: [wire_v1] at w1.2, [wire_v2] at w2.1 and [wire_v3] at w3.0, one API root each, their wire versions in
//! `Cargo.toml`'s `[package.metadata.wire_weaver.roots]`. What more than one line uses unchanged is in [common], one
//! Rust type for every line that uses it.
//!
//! The lines were the crates 1.x, 2.x and 3.x, each with its root at the crate root; their snapshots are in
//! `api_snapshots/`, and each root of this crate is checked against its own line's latest one (`tests/evolution.rs`).
//! Firmware built from those crates keeps working with a host built from this one: `tests/thermo_api_old` keeps
//! 1.0.0 and 2.0.0, the first revision of lines 1 and 2, as such firmware.
//!
//! It touches every place a wire break can bite: methods with arguments, returns and errors, properties with get,
//! set and observe, an enum, a stream up and a sink down, an array of sub-levels, items added within a line
//! (`#[since]`), unit types from `ww_si` and a trait from `ww_stdlib` (`ww_indication`) every line shares.
#![cfg_attr(not(feature = "std"), no_std)]

pub mod common;
pub mod wire_v1;
pub mod wire_v2;
pub mod wire_v3;

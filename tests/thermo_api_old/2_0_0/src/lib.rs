//! The API of the multi-line bench (EVO-28, EVO-42), a small temperature logger, with every wire line still in use in
//! one crate: [wire_v1] at w1.0 and [wire_v2] at w2.0, one API root each, their wire versions in `Cargo.toml`'s
//! `[package.metadata.wire_weaver.roots]`. What more than one line uses unchanged is in [common], one Rust type for
//! every line that uses it.
//!
//! It touches every place a wire break can bite: methods with arguments, returns and errors, properties with get,
//! set and observe, an enum, a stream up and a sink down, an array of sub-levels, items added within a line
//! (`#[since]`), unit types from `ww_si` and a local `common::Indication` (a stand-in for a `ww_stdlib` trait) every
//! line shares.
#![cfg_attr(not(feature = "std"), no_std)]

pub mod common;
pub mod wire_v1;
pub mod wire_v2;

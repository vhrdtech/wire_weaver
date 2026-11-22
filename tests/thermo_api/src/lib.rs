//! The API of the multi-line bench (EVO-28, EVO-42), a small temperature logger, with every wire line still in use in
//! one crate: [wire_v2] at w2.1 and [wire_v3] at w3.0, one API root each, their wire versions in `Cargo.toml`'s
//! `[package.metadata.wire_weaver.roots]`. Line 1 (`wire_v1`, w1.2) is retired: its module stays behind the
//! `retired_wire_v1` feature. What more than one line uses unchanged is in [common], one Rust type for every line
//! that uses it.
//!
//! The history: 0.1.0 on `w0`, the design line, which keeps no snapshot; 1.0.0 the first release, line 1 at w1.0
//! with its root at the crate root; 2.0.0 added line 2 (w2.0) beside it, one module per root; 2.1.0 and 2.2.0 gave
//! line 1 `calibrate` (w1.1) and `uptime` (w1.2), 2.3.0 gave line 2 `self_test` (w2.1); 3.0.0 added line 3 (w3.0);
//! 4.0.0 retired line 1 (`ww api line retire`). The snapshots are in `api_snapshots/`, and each root of this crate is
//! checked against its own line's latest one (`tests/evolution.rs`). Firmware built from an older revision of a line
//! keeps working with a host built from this one: `tests/thermo_api_old/2_0_0` keeps 2.0.0, the first revisions of
//! lines 1 and 2, as such firmware.
//!
//! It touches every place a wire break can bite: methods with arguments, returns and errors, properties with get,
//! set and observe, an enum, a stream up and a sink down, an array of sub-levels, items added within a line
//! (`#[since]`), unit types from `ww_si` and a local `common::Indication` (a stand-in for a `ww_stdlib` trait) every
//! line shares.
#![cfg_attr(not(feature = "std"), no_std)]

pub mod common;
#[cfg(feature = "retired_wire_v1")]
pub mod wire_v1;
pub mod wire_v2;
pub mod wire_v3;

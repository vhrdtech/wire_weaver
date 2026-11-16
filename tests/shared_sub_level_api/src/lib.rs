//! Two wire lines of one API in one crate (EVO-37) that reach the same sub-traits at different places: the crate's
//! own `common::Channel` and `common::Indication` (standing in for a real indication trait, R7). Used by
//! `tests/shared_sub_level`, where one client of each trait serves both lines (CLT-68).
#![no_std]

pub mod common;
pub mod wire_v1;
pub mod wire_v2;

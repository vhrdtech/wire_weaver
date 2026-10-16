//! An API with `#[cfg]` enum variants (DER-8). `tests/cfg_variants_narrow_api` has the same source, built without
//! the `wide` and `float` features, as a firmware would; `tests/cfg_variants` connects a host built with every
//! variant to it.
//!
//! In a real project both are the same crate with different features; a workspace unifies features, so the
//! firmware's build is kept under its own name, and the test device reports it as `cfg_variants_api`.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait CfgVariants {
    /// Last reading written, `Reading::Small(0)` at first
    fn read() -> Reading;
    fn write(reading: Reading);
    /// Supply voltage, `ww_si::Volt` is a skipped type known from the `ww_si` snapshot, checked by signature
    fn supply() -> ww_si::Volt;
}

/// A value whose bigger variants only some builds have.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq),
    non_exhaustive,
    self_describing,
    ww_repr = unib32
)]
pub enum Reading {
    Small(u8),
    #[cfg(feature = "wide")]
    Wide(u64),
    #[cfg(feature = "float")]
    Float(f32),
    Missing,
}

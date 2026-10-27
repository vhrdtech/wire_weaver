//! Test API using the types of `standalone_units`, a dependency that isn't a wire_weaver API crate (API-60). Used
//! by `tests/standalone`.
#![cfg_attr(not(feature = "std"), no_std)]

/// Re-exported, so crates depending on this one name its types (and generated code in them does) without a
/// dependency of their own.
pub use standalone_units;
use standalone_units::units::{Ampere, Volt};
use standalone_units::{Milli, Source};
use wire_weaver::prelude::*;

#[ww_api_root]
pub trait Standalone {
    /// Supply voltage and how it was found.
    fn supply() -> Supply;
    fn set_limit(current: Milli<Ampere>);
    property!(ro limit: Milli<Ampere>);
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct Supply {
    pub voltage: Milli<Volt>,
    pub source: Source,
}

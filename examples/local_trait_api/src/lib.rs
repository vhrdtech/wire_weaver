//! An API used two ways (CLT-57): through a device or service with the generated client, and in the same process
//! by a plain struct, with one call site generic over the [Counter] trait generated below.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait CounterApi {
    /// Adds `by` times `step` to the total, returns the new total and sends it on `totals`
    fn add(by: u32) -> u32;
    /// The total divided by `parts`, `Err(CounterError::DivByZero)` for 0 parts
    fn split(parts: u32) -> Result<u32, CounterError>;
    /// The counter's state, with its name
    fn snapshot() -> Snapshot<'i>;
    /// Added per unit of `add`
    property!(rw step: u32);
    /// Every new total
    stream!(totals: u32);
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone))]
pub struct Snapshot<'i> {
    pub name: &'i str,
    pub total: u32,
    pub step: u32,
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Eq), ww_repr = u4)]
pub enum CounterError {
    DivByZero,
}

impl core::fmt::Display for CounterError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CounterError::DivByZero => write!(f, "division by zero"),
            CounterError::Unknown(n) => write!(f, "unknown error {n}"),
        }
    }
}

impl core::error::Error for CounterError {}

/// `Counter`, the API as an async trait, and `CounterDyn` for trait objects.
#[cfg(feature = "local")]
mod local {
    wire_weaver::ww_codegen!(
        crate::CounterApi,
        local_trait = "Counter",
        // debug_to_file = "../../target/local_trait_api_trait.rs"
    );
}
#[cfg(feature = "local")]
pub use local::{Counter, CounterDyn};

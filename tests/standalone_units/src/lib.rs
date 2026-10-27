//! A crate that isn't built on wire_weaver (API-60): no `wire_weaver` dependency, no `#[ww_trait]`, only data types
//! with `shrink_wrap` as a plain dependency, the way `exact_units` is. `tests/standalone_api` uses its types in an
//! API, the crate walker walks it like any other dependency.
#![cfg_attr(not(feature = "std"), no_std)]

use core::marker::PhantomData;
use shrink_wrap::prelude::*;

/// A unit of measure, named only in types.
pub trait Unit {}

pub mod units {
    /// Volts.
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub struct Volt;
    impl super::Unit for Volt {}

    /// Amperes.
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub struct Ampere;
    impl super::Unit for Ampere {}
}

/// A value of `U` in thousandths, the unit fixed at compile time.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    sized
)]
pub struct Milli<U: Unit> {
    pub raw: i32,
    _unit: PhantomData<U>,
}

impl<U: Unit> Milli<U> {
    pub const fn new(raw: i32) -> Self {
        Self {
            raw,
            _unit: PhantomData,
        }
    }
}

/// How a reading was taken.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = u4
)]
pub enum Source {
    Measured,
    Estimated { confidence: u8 },
}

//! A crate that isn't built on wire_weaver (API-60): no `wire_weaver` dependency, no `#[ww_trait]`, only data types
//! with `shrink_wrap` as an optional dependency, the way `exact_units` is: each derive is behind
//! `#[cfg_attr(feature = "shrink_wrap", ..)]`. `tests/standalone_api` uses its types in an API and turns the feature
//! on, the crate walker reads them with it.
#![cfg_attr(not(feature = "std"), no_std)]

use core::marker::PhantomData;
#[cfg(feature = "shrink_wrap")]
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
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(
    feature = "shrink_wrap",
    derive_shrink_wrap(borrowed, owned(feature = "std"), sized)
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
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(
    feature = "shrink_wrap",
    derive_shrink_wrap(borrowed, owned(feature = "std"), self_describing, ww_repr = u4)
)]
pub enum Source {
    Measured,
    Estimated { confidence: u8 },
}

/// A reading with fields added later, their defaults behind the feature as well.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(
    feature = "shrink_wrap",
    derive_shrink_wrap(borrowed, owned(feature = "std"))
)]
pub struct Reading {
    pub value: Milli<units::Volt>,
    #[cfg_attr(feature = "shrink_wrap", default = 1)]
    pub samples: u8,
}

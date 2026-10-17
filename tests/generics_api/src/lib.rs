//! Generic user types in an API (API-43): type and const parameters, generic enums, a lifetime next to a parameter,
//! a const parameter passed on, and `ww_si::Timed<T>` from a snapshot, as method arguments and return values,
//! properties and stream items. Type aliases (API-45), plain and generic, as a method argument and return value, a
//! property and inside `Timed`, named like its unit marker in a module (ww_si 0.2 layout, API-46).
#![cfg_attr(not(feature = "std"), no_std)]

use core::marker::PhantomData;
use wire_weaver::prelude::*;
use ww_si::{Ampere, Timed};

#[ww_trait]
trait Generics {
    /// The two values swapped
    fn swap(pair: Pair<u8, u16>) -> Pair<u16, u8>;
    /// The tag's length and the value
    fn tagged(tagged: Tagged<'i, Reading<u32>>) -> Pair<u32, Reading<u32>>;
    /// Sum of the samples
    fn sum(window: Window<4>) -> u32;
    property!(rw latest: Timed<Ampere>);
    property!(rw outer: Outer<3>);
    stream!(readings: Reading<i16>);
    /// The voltage doubled
    fn double(v: Volt) -> Volt;
    property!(rw voltage: Volt);
    property!(rw timed_voltage: Timed<Volt>);
    property!(rw sample: Sample<u8>);
}

/// A value of the unit `U`, a stand-in for `ww_si::Quantity<U>`: the unit is only in the type (`PhantomData`, DER-15)
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Clone, Copy)
)]
pub struct Quantity<U> {
    pub value: i32,
    _unit: PhantomData<U>,
}

impl<U> Quantity<U> {
    pub const fn new(value: i32) -> Self {
        Self {
            value,
            _unit: PhantomData,
        }
    }
}

/// Unit markers, as `ww_si::units` has them: a marker and the alias of its quantity share a name (API-46)
pub mod units {
    /// Volts, a marker without serdes impls
    #[derive(Debug, PartialEq, Clone, Copy)]
    pub struct Volt;
}

/// A plain alias of a generic type, named like the marker in `units`
pub type Volt = Quantity<units::Volt>;

/// A generic alias: its parameter goes into the types it names
pub type Sample<T> = Pair<Window<2>, Reading<T>>;

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Clone, Copy)
)]
pub struct Pair<A, B> {
    pub a: A,
    pub b: B,
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Clone))]
pub struct Tagged<'i, T> {
    pub tag: &'i str,
    pub value: T,
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Clone, Copy),
    ww_repr = u4
)]
pub enum Reading<T> {
    Missing,
    Value(T),
    Range { lo: T, hi: T },
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Clone, Copy)
)]
pub struct Window<const N: usize> {
    pub samples: [u16; N],
}

/// A const parameter passed on to another generic type
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Clone, Copy)
)]
pub struct Outer<const N: usize> {
    pub id: u8,
    pub window: Window<N>,
}

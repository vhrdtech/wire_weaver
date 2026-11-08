//! A second API crate with a unit of its own named like one of `quantities_api`'s: its `Rpm` is the same unit to a
//! reader's eye (symbol, dimension, scale) and yet another unit, `fan_api::Rpm`, by its identity (UNIT-7). An API
//! that uses both lists both in its model, and a dynamic client resolves a reference to each.
#![cfg_attr(not(feature = "std"), no_std)]

ww_si::unit! {
    /// A fan's revolutions per minute: π/30 rad/s
    pub struct Rpm = "rpm", [rad 1, s -1], Exact(1 / 30, pi 1);
}

//! Running totals over plain sinks (API-70): `Totals<Delta>` of a struct of two `Fixed` values, which implements
//! `Accumulate` field by field, and `Totals<Fixed<Detent, One, i32>>` of a counted unit of this crate.
#![cfg_attr(not(feature = "std"), no_std)]

use ww_api::prelude::*;
use ww_si::{Fixed, prefix::One};

#[ww_api_root]
#[effect(edit)]
trait Pointer {
    /// Moves the pointer by the change of the totals since the last write applied.
    sink!(motion: Totals<Delta>);
    /// Scrolls by the change of the totals since the last write applied.
    sink!(scroll: Totals<Detents>);
}

ww_si::unit! {
    /// A logical pixel, a counted base of this crate.
    pub struct Pixel = "px", base;
    /// A notch of a scroll wheel.
    pub struct Detent = "detent", base;
}

/// A distance in logical pixels.
pub type Px = Fixed<Pixel, One, i32>;

/// A number of scroll wheel notches.
pub type Detents = Fixed<Detent, One, i32>;

/// A pointer move: how far right and down.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, Default, PartialEq, Eq),
    final_structure
)]
pub struct Delta {
    pub dx: Px,
    pub dy: Px,
}

impl Accumulate for Delta {
    fn accumulate(self, change: Self) -> Self {
        Self {
            dx: self.dx.accumulate(change.dx),
            dy: self.dy.accumulate(change.dy),
        }
    }

    fn since(self, earlier: Self) -> Self {
        Self {
            dx: self.dx.since(earlier.dx),
            dy: self.dy.since(earlier.dy),
        }
    }
}

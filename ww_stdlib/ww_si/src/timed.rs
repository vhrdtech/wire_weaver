//! [Timed]: a value with the time it was taken.

use crate::Second;
use shrink_wrap::prelude::*;

/// A value with the time it was taken: a sample of a property, an item of a stream (`Timed<Ampere>`,
/// `Timed<IvSample>`).
///
/// `t` is a device timestamp, as [Second]'s docs say: the device's time since its clock started, at a prefix and
/// storage that stay the same for a given property or stream (`Second { Micro, U32 }` wraps every 71.6 minutes, the
/// host gets the full time back with [Unwrapper](crate::Unwrapper)).
///
/// On the wire it is a `final_structure` struct of `t` and then `value`: no fields can be added later. In a
/// `Series` (`series`, SW-60) `t` is a column like any other: a regular clock costs nothing per item.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq),
    final_structure,
    series
)]
pub struct Timed<T> {
    /// When `value` was taken, device time.
    pub t: Second,
    pub value: T,
}

impl<T> Timed<T> {
    pub const fn new(t: Second, value: T) -> Self {
        Timed { t, value }
    }
}

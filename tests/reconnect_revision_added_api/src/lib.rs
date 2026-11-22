//! Revision w1.1 of `reconnect_revision_api` (CLT-81): the same resources and one method more, compatible with a
//! host of w1.0.
use ww_api::prelude::*;
use ww_si::Fixed;
use ww_si::prefix::One;
use ww_si::units::Item;

#[ww_api_root]
trait Revision {
    /// Counts up on the device every millisecond
    property!(ro+observe ticks: Fixed<Item, One, u32>);
    /// Bytes, as a counted stream; the same in every revision
    #[counted]
    stream!(kept: [u8]);
    /// Readings, as a counted stream; unit: item
    #[counted]
    stream!(readings: Fixed<Item, One, u8>);
    /// Device sends one event on each open stream
    fn emit();
    /// Added in w1.1
    #[since = "w1.1"]
    fn added();
}

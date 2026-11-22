//! Revision w1.1 of `reconnect_revision_api` (CLT-81) that changes the item type of `readings`, against the evolution
//! rules: a host of w1.0 can't take that stream any more, the rest still holds.
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
    stream!(readings: Fixed<Item, One, u32>);
    /// Device sends one event on each open stream
    fn emit();
}

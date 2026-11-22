//! API of the reconnect-onto-a-new-revision tests (CLT-81), wire w1.0: a host of it reconnects to a device of
//! `reconnect_revision_added_api` (w1.1, a method added) and of `reconnect_revision_changed_api` (w1.1, the item type
//! of `readings` changed). In a real project the three are one crate at three versions.
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
}

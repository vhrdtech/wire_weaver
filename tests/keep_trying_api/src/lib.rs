//! API of the keep-trying connection tests (CLT-75): one observed property the device counts up, which a host keeps
//! observing while the device starts late, goes away and comes back.
use ww_api::prelude::*;
use ww_si::Fixed;
use ww_si::prefix::One;
use ww_si::units::Item;

#[ww_api_root]
trait Counter {
    /// Counts up on the device every millisecond
    property!(ro+observe ticks: Fixed<Item, One, u32>);
}

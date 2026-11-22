//! Stands in for a real `ww_stdlib` crate (R7) in `wire_weaver_client`'s EVO-26 tests: a sub-trait whose `#[since]`
//! names a revision of *this crate's own* wire line (`w2.0`), not its API root's.
#![cfg_attr(not(feature = "std"), no_std)]

use ww_api::prelude::*;

#[ww_trait]
#[effect(edit)]
pub trait Fixture {
    /// Added on this crate's wire `w1.2`, like a real stdlib crate's revision marker.
    #[since = "w1.2"]
    fn set_mode(mode: u8);
}

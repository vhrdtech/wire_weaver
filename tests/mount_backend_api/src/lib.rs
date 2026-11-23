//! The backend mounted in the mount forwarder tests (SRV-78): what a hub process serves behind a TV's hub slot.
#![cfg_attr(not(feature = "std"), no_std)]

use ww_api::prelude::*;

#[ww_api_root]
#[effect(edit)]
trait Hub {
    /// Adds two numbers: a plain round trip through the forwarder.
    fn add(a: u32, b: u32) -> u32;
    /// Never answered while the backend runs: a request in flight when the backend goes away.
    fn hang();
    /// A trait the TV doesn't have, reached by a global path through the TV.
    ww_impl!(lamp: Lamp);
}

/// A lamp in the hub's room.
#[ww_trait]
#[effect(edit)]
pub trait Lamp {
    /// Switches the lamp and says whether it is on now.
    fn toggle() -> bool;
}

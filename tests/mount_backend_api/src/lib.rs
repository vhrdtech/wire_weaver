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
    /// Ticks of the hub: a stream open through the forwarder when the backend goes away.
    stream!(ticks: [u8]);
    /// Answers twice: first under `seq`, a seq it was never sent, then under its own. `seq` is a raw wire seq.
    fn stray(seq: u32);
}

/// A lamp in the hub's room.
#[ww_trait]
#[effect(edit)]
pub trait Lamp {
    /// Switches the lamp and says whether it is on now.
    fn toggle() -> bool;
    /// Whether the lamp is on: observed through the forwarder when the backend goes away.
    property!(ro+observe on: bool);
}

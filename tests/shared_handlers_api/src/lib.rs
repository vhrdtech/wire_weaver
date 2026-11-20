//! Test API of shared handlers (SRV-65, `receiver_model`): a shared item next to an exclusive one at the root, in a
//! level, in an array of levels, in a keyed array and in a global trait, so that `tests/shared_handlers` checks both
//! dispatch trees and `is_shared` on every path shape. The numbers are plain test values: the test is about
//! dispatch, not units.
#![cfg_attr(not(feature = "std"), no_std)]

use ww_api::prelude::*;

#[ww_api_root]
trait SharedHandlers {
    /// Shared: a call with arguments and a result.
    fn add(a: u32, b: u32) -> u32;
    /// Shared and deferred: answered later through `add_later_send_return`.
    fn add_later(a: u32, b: u32) -> u32;
    /// Exclusive: changes the server.
    fn bump();
    /// Shared read.
    property!(ro status: u32);
    /// Read shared, written exclusively.
    property!(rw counter: u32);
    /// Exclusive whatever the model says: streams are never shared.
    stream!(events: u32);
    /// A level with a shared and an exclusive item.
    ww_impl!(sensors: Sensors);
    /// A level with nothing shared: no `&self` dispatch for it.
    ww_impl!(motor: Motor);
    /// An array of levels with shared items.
    ww_impl!(channel[]: Channel);
    /// A keyed array, its elements' reads and the key companion shared.
    ww_impl!(flow[]: Flow);
    /// Flows by key.
    keyed!(flow by FlowKey);
    /// A trait also reached by a global path.
    ww_impl!(info: global_fixture_api::GlobalDemo);
}

#[ww_trait]
trait Sensors {
    /// Shared read.
    property!(ro temp: i32);
    /// Exclusive: changes the level.
    fn calibrate(offset: i32) -> i32;
}

#[ww_trait]
trait Motor {
    /// Exclusive read and write.
    property!(rw speed: u32);
}

#[ww_trait]
trait Channel {
    /// Shared read of an element.
    property!(ro level: u32);
    /// Shared call on an element.
    fn scaled(by: u32) -> u32;
}

#[ww_trait]
trait Flow {
    /// Shared read of a keyed element.
    property!(ro packets: u32);
}

/// What identifies a flow.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone, Copy)
)]
pub struct FlowKey(pub u16);

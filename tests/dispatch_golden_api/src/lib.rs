//! Test API of TEST-31 (golden reply bytes of the dispatcher): one resource of every kind and outcome the
//! dispatcher serves, used by `tests/dispatch_golden`'s corpus of request bytes -> golden reply bytes. The reply
//! bytes are wire-defining, this crate and the servers built on it are not: a rework of the dispatcher (SRV-51,
//! SRV-52, SRV-53) must leave the corpus green.
#![cfg_attr(not(feature = "std"), no_std)]

use ww_api::prelude::*;

#[ww_api_root]
pub trait DispatchGoldenTest {
    /// `Call` with no arguments and no return value: the smallest case.
    fn no_args();
    /// `Call` with arguments and a return value.
    fn with_args(a: u8, b: u16) -> u32;
    /// The request is fine, there is no handler for it: `Unimplemented`.
    fn absent();
    /// Answered later, from outside the handler, by `answer_deferred`.
    fn deferred() -> u8;
    /// Answers the pending `deferred` call with `value`.
    fn answer_deferred(value: u8);

    /// Plain read/write property; a write of 255 is refused with the common error `Refused` (STD-16).
    #[errors(CommonError(Refused))]
    property!(rw plain: u8);
    /// A property whose setter can fail with a user error.
    #[errors(TooBig)]
    property!(rw guarded: u8, GuardedError);

    /// A Rust const: read once per connection by a real client, by a plain `Read` here.
    const LIMIT: u16;

    /// A stream the handler pushes into.
    stream!(samples: u8);
    /// A sink the host writes into.
    sink!(commands: u8);

    /// An array of a nested trait: an index chain in the path.
    ww_impl!(channel[]: Channel);

    /// A global trait, reached by `GlobalFull` and `GlobalCompact`: also covers a property write and a const read
    /// on a global path.
    ww_impl!(info: global_fixture_api::GlobalDemo);

    /// `Call` of a method without an error type whose handler refuses: the common error event (STD-16).
    #[errors(CommonError(Busy))]
    fn busy();
    /// `Call` of a method with an error type of its own: `pick` 0 is `Ok(7)`, 1 its own `Err(TooBig)` in the
    /// value, 2 the common error event `InUse` (STD-16).
    #[errors(CommonError(InUse), TooBig)]
    fn checked(pick: u8) -> Result<u8, GuardedError>;
}

#[ww_trait]
pub trait Channel {
    /// Called on one index of the array.
    fn pulse() -> u8;
    property!(rw level: u8);
}

/// Error of [DispatchGoldenTest::guarded]'s setter.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone, Copy),
    ww_repr = u4
)]
pub enum GuardedError {
    /// Above 100.
    TooBig,
}

ww_api::api_error!(GuardedError);

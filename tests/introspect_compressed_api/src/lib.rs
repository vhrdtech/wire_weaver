//! Test API for compressed introspection data (INT-16), used by `tests/introspect_compressed`. Its doc comments
//! are what makes introspection data large on real devices, and they are long enough here for the compressed bytes
//! to take more than one introspection chunk (128 bytes).
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait IntrospectCompressed {
    /// Turns the output of a channel on when it was off and off when it was on, and returns whether it is on now.
    /// A channel that doesn't exist is left alone and reads as off.
    fn toggle(channel: ChannelId) -> bool;

    /// Turns every output off at once, the same as toggling each channel that is on.
    fn all_off();

    /// Whether the outputs follow the toggles at all: while this is off, toggles are remembered and take effect
    /// when it is turned on again.
    property!(rw enabled: bool);
}

/// One of the device's output channels, counted from 0.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct ChannelId(pub u8);

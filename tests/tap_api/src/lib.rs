//! Test API of the link tap's CLI tests (`ww monitor`, `ww capture`, `ww explain FILE`, TOOL-14).
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait TapDemo {
    /// `a + b`, wrapping
    fn add(a: u32, b: u32) -> u32;
    /// `len` bytes counting up from 0: a long reply is split over several USB packets
    fn blob(len: u16) -> RefVec<'i, u8>;
    property!(rw speed: u16);
    /// Changes on every poll of the device, observed
    property!(ro+observe load: u16);
    /// Observed together with `load`: an update carries both
    property!(ro+observe state: u8);
    /// A global trait (known from a snapshot): the device leaves it out of its introspection data
    ww_impl!(user: ww_user_info::UserInfo);
}

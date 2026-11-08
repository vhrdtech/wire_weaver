//! Test API with `const` items (API-36), used by `tests/consts`.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait ConstsTest {
    /// Known at compile time: a Rust const on the server.
    const LIMITS: Limits<'i>;
    /// Fixed at boot (read from the chip): a `fn` on the server.
    const SERIAL: &'i str;
    /// Left out by some devices.
    #[optional]
    const EXTRA: u8;
    /// Not a const, read from the device each time.
    property!(ro uptime: u32);

    ww_impl!(info: Info);
    ww_impl!(channel[]: Channel);
}

#[ww_trait]
pub trait Info {
    const NAME: &'i str;
}

#[ww_trait]
pub trait Channel {
    /// Differs per channel: a `fn` with the index.
    const GAIN: u8;
    /// Same for all channels.
    const KIND: &'i str;
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq))]
pub struct Limits<'i> {
    pub max_len: u16,
    pub names: RefVec<'i, &'i str>,
}

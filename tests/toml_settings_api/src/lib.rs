//! A settings tree as an application keeps it in TOML level files, loaded from source by `ww_json`'s TOML tests
//! (INT-18): nested structs, quantities, unit and data enums, a list of strings, options and a wide integer.
#![cfg_attr(not(feature = "std"), no_std)]

use ww_api::prelude::*;
use ww_si::prefix::{Centi, Kilo, Milli, One};
use ww_si::units::{Hertz, Item, Ratio, Second};
use ww_si::{Fixed, Range};

#[ww_api_root]
#[effect(persist)]
pub trait SettingsTest {
    property!(rw settings: Settings<'i>);
}

/// Every setting, one struct per area.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct Settings<'i> {
    pub audio: Audio,
    pub swear: Swear<'i>,
    pub seed: Seed,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct Audio {
    pub window: Fixed<Second, Milli, u16>,
    pub rate: Fixed<Hertz, Kilo, u16>,
    pub duty: Fixed<Ratio, Centi, u8, Range<0, 100>>,
    pub gain: f32,
    pub mode: Mode,
    pub limits: Option<Limits>,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct Limits {
    pub items: Fixed<Item, One, u16>,
    pub strict: bool,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Clone, Debug, PartialEq), ww_repr = u4)]
pub enum Mode {
    FastStart,
    Off,
    Custom { level: u8 },
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct Swear<'i> {
    pub tokens: RefVec<'i, &'i str>,
    pub enabled: bool,
    pub action: Option<Action>,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Clone, Debug, PartialEq), ww_repr = u4)]
pub enum Action {
    Beep,
    Mute,
}

/// A random seed, every `u64` value valid.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct Seed(pub u64);

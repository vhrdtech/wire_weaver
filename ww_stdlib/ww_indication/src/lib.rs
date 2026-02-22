#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_trait]
pub trait Indication {
    /// Global operation mode.
    property!(mode: Mode);

    /// Brightness control for informational indicators.
    property!(info_brightness: Brightness);

    /// Brightness control for alert indicators or double use indicators.
    property!(alert_brightness: AlertBrightness);

    /// Global animation enable, optional to implement.
    property!(animation_enable: bool);

    /// Must automatically leave test mode after 5 seconds.
    property!(test_mode: TestMode<'i>);

    /// True if some indicators are hard-wired to power.
    const HAS_NOT_CONTROLLABLE: bool;

    /// Indicator names: can be left empty for conserving memory.
    const NAMES: RefVec<'i, str>;
}

#[derive_shrink_wrap]
#[ww_repr(u4)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Mode {
    AllOff,
    OnlyEssential,
    Normal,
}

/// Indicators brightness mode
#[derive_shrink_wrap]
#[ww_repr(u4)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Brightness {
    /// Night mode, non-distracting
    Lowest,
    /// Day mode, non-distracting
    Low,
    /// Good visibility inside
    Normal,
    /// For alerts inside
    High,
    /// Visible outside
    Highest,
}

/// Alert indicators brightness mode, relative to info indicators.
/// (alert indicators cannot have brightness lower than info ones)
#[derive_shrink_wrap]
#[ww_repr(u4)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum AlertBrightness {
    /// Highest setting of [Brightness](Brightness)
    Max,
    P0,
    P1,
    P2,
    P3,
    P4,
}

#[derive_shrink_wrap]
#[ww_repr(u4)]
#[owned = "std"]
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum TestMode<'name> {
    Off,
    AllOn,
    AllOff,
    AllBlink,
    SequentialBlink,
    IndividualOn(&'name str),
    IndividualBlink(&'name str),
}

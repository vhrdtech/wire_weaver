//! API of the nested levels and resource arrays of the `serve_trait` tests (CLT-58, `tests/serve_trait`): each level
//! names its local trait on its definition, `#[ww_trait(local = "..")]`, and the root's comes from `ww_codegen!`.
use ww_api::prelude::*;

#[ww_api_root]
#[effect(edit)]
pub trait PlantApi {
    /// A nested level
    ww_impl!(desk: DeskApi);
    /// Units by index
    ww_impl!(unit[]: UnitApi);
    /// Units by their tag
    keyed!(unit by Tag);
    /// A level from another crate
    ww_impl!(gauge: serve_trait_gauge_api::GaugeApi);
    /// One level per slot
    property!(rw slot[]: Level);
    /// The slot's level, then its double
    stream!(feed[]: Level);
}

#[ww_trait(local = "Desk")]
#[effect(edit)]
pub trait DeskApi {
    /// `n` plus one
    fn ping(n: Level) -> Level;
    property!(rw lamp: Level);
}

#[ww_trait(local = "Unit")]
#[effect(edit)]
pub trait UnitApi {
    /// The unit's index
    #[effect(read)]
    fn id() -> Level;
    property!(rw level: Level);
    /// `count` ticks from the unit's id, `Err(PlantError::Refused)` for 0
    #[stream(Level)]
    fn ticks(count: Level) -> Result<Level, PlantError>;
    /// Channels by their own index type
    ww_impl!(channel[ChannelIdx]: ChannelApi);
    /// Channels by their level
    keyed!(channel by Level);
}

#[ww_trait(local = "Channel")]
#[effect(edit)]
pub trait ChannelApi {
    /// `by` times the channel's index plus one, `Err(PlantError::Refused)` for 0
    fn gain(by: Level) -> Result<Level, PlantError>;
}

/// A level, of anything.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq)
)]
pub struct Level(pub u32);

/// What names a unit, whatever its index.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq)
)]
pub struct Tag(pub u32);

/// The index of a channel: a typed index converts into `u32` for the client and back for `serve_trait`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq)
)]
pub struct ChannelIdx(pub u8);

impl From<ChannelIdx> for u32 {
    fn from(index: ChannelIdx) -> u32 {
        u32::from(index.0)
    }
}

impl TryFrom<u32> for ChannelIdx {
    type Error = core::num::TryFromIntError;

    fn try_from(index: u32) -> Result<Self, Self::Error> {
        u8::try_from(index).map(ChannelIdx)
    }
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq), ww_repr = u4)]
pub enum PlantError {
    Refused,
}

ww_api::api_error!(PlantError);

impl core::fmt::Display for PlantError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PlantError::Refused => write!(f, "refused"),
            PlantError::Unknown(n) => write!(f, "unknown error {n}"),
        }
    }
}

impl core::error::Error for PlantError {}

/// `Plant` and the traits of the levels of this crate: `Desk`, `Unit` and `Channel`.
#[cfg(feature = "local")]
mod local {
    wire_weaver::ww_codegen!(crate::PlantApi, local_trait = "Plant");
}
#[cfg(feature = "local")]
pub use local::{Channel, ChannelDyn, Desk, DeskDyn, Plant, PlantDyn, Unit, UnitDyn};

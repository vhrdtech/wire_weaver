use wire_weaver::prelude::*;
use ww_si::Fixed;
use ww_si::prefix::One;
use ww_si::units;

/// Errors of the CAN traits: the common ones plus what is specific to CAN; each method lists the variants it
/// returns.
#[derive_shrink_wrap(
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone),
    ww_repr = unib32,
    display
)]
pub enum CanError<'i> {
    /// {0}
    #[common]
    Common(CommonError<'i>),
    /// More filters than the controller has ({max} of this kind).
    OutOfFilters { max: Fixed<units::Filter, One, u16> },
    /// Not possible while the controller is bus-off.
    BusOff,
    /// Doesn't fit the controller's memory ({available} left).
    OutOfMemory {
        available: Fixed<units::Item, One, u32>,
    },
}

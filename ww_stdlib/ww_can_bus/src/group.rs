use wire_weaver::prelude::*;
use ww_si::prefix::One;
use ww_si::units::{self, Per};
use ww_si::{Fixed, Hertz, Index};

use crate::Can;
use crate::error::CanError;

/// Most clock sources a [CanGroup] lists.
pub const MAX_CLOCK_SOURCES: usize = 8;

/// Longest clock source name.
pub const MAX_NAME_LEN: usize = 32;

/// Controllers that share a clock, power and message memory, as all FDCAN instances of an STM32: none of them can be
/// clocked, powered down or re-laid out alone. The group is enabled (clocked) or disabled as a whole; each
/// controller is `interface[i]`, configured on its own once the group is enabled.
#[ww_trait]
#[claim(exclusive)]
pub trait CanGroup {
    /// The controllers of the group.
    ww_impl!(interface[]: Can);

    /// What the group has: number of controllers, clock sources and frequencies, shared message memory.
    fn group_capabilities() -> GroupCapabilities<'i>;

    /// Enables the group's clock (the source and frequency set last, or the device's default) and answers the clock
    /// in effect. Every controller is then in [Mode::Off] until configured. `WrongState` when already enabled.
    #[errors(Common(WrongState, NoPower))]
    fn enable() -> Result<GroupClock, CanError<'i>>;

    /// Disables the group's clock, its lowest power state. Every controller must be in [Mode::Off] first
    /// (`WrongState` names the one that isn't).
    #[errors(Common(WrongState))]
    fn disable() -> Result<(), CanError<'i>>;

    /// Enabled (with the clock in effect) or disabled; sent to observers on every change.
    property!(ro group_state: GroupState);

    /// Selects the group's clock source and frequency, and answers what was set (the nearest frequency the source's
    /// dividers reach). Only while disabled (`WrongState`); a source or frequency the group doesn't have is
    /// `Unsupported`. For advanced nodes that pick a clock to hit an exact bit timing; a bridge uses the default.
    #[optional(clock)]
    #[errors(Common(Unsupported, WrongState))]
    fn set_clock(clock: GroupClock) -> Result<GroupClock, CanError<'i>>;

    /// What the group guarantees together, under the current clock and system load: a device whose controllers
    /// share a bus to the host or a CPU knows it, the host can't.
    #[optional]
    fn performance() -> Performance;
}

/// Throughput a controller or group guarantees, see `CanGroup::performance` and [Capabilities::performance].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct Performance {
    /// Frames received and passed on without loss, at any frame length.
    pub min_frame_rate: Option<Fixed<Per<units::Frame, units::Second>, One, u32>>,
    /// Data passed on without loss.
    pub min_data_rate: Option<Fixed<Per<units::Byte, units::Second>, One, u32>>,
    /// The figures hold whatever else the device does (dedicated DMA, a core of its own).
    pub independent_of_system: bool,
}

/// Clock of a [CanGroup]: a source and the frequency the controllers get from it.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq)
)]
pub struct GroupClock {
    /// Its place in [GroupCapabilities::clock_sources].
    pub source: Index<units::Item, u8>,
    /// Frequency the controllers get (after the group's divider).
    pub frequency: Hertz,
}

/// Enabled or disabled, see `CanGroup::group_state`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq),
    ww_repr = nib
)]
pub enum GroupState {
    /// Clock off, the lowest power state.
    Disabled,
    Enabled(GroupClock),
}

/// A clock source of a [CanGroup].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct ClockSource<'i> {
    /// "HSE", "PLL1Q", "PLL2P".
    pub name: MaxLen<&'i str, MAX_NAME_LEN>,
    /// Lowest frequency the group's dividers make of it.
    pub min: Hertz,
    /// Highest one.
    pub max: Hertz,
}

/// What a [CanGroup] has, see `CanGroup::group_capabilities`.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct GroupCapabilities<'i> {
    /// Controllers in `interface[]`.
    pub interfaces: Fixed<units::Item, One, u8>,
    /// Clock sources `CanGroup::set_clock` chooses from; empty when the clock is fixed.
    pub clock_sources: MaxLen<RefVec<'i, ClockSource<'i>>, MAX_CLOCK_SOURCES>,
    /// The clock `CanGroup::enable` uses without `set_clock`.
    pub default_clock: GroupClock,
    /// Message memory shared by the controllers; `None` when each has its own.
    pub shared_memory: Option<Fixed<units::Word, One, u32>>,
}

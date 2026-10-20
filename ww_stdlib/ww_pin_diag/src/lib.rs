#![cfg_attr(not(feature = "std"), no_std)]
//! Raw access to a board's digital points for bring-up, verification and production tests (STD-47).
//!
//! A *point* is anything digital the firmware lets a test touch: an MCU pin, an output of a shift register or an
//! I/O expander. The firmware lists its points in [PinDiag::POINTS] with what each can do; a host sets and reads them
//! one call at a time (the framer packs many small calls into one frame) and finds shorts to GND, to the supply and
//! between points, external pull resistors and floating pins, or which pins follow an expander output.
//!
//! How a point is made, and what is safe to do with it, stays in the firmware. A shift register output is a point
//! like any other: the host asks for a level, the firmware checks its board's rules (interlocks between outputs, pins
//! a function holds, pins that must never drive) and does it or refuses. So a board's safety lives where the board
//! knowledge is, and the trait only carries the outcome: [PinDiagError::NotCapable] for what a point can never do,
//! `Common(InUse)` for a point a function holds, [PinDiagError::Refused] with the rule's name for what the board's
//! rules forbid now.
//!
//! The drive strength and slew rate of an output are the firmware's choice, except that [Drive::Weak] asks for the
//! weakest drive the point has: a short-to-GND test drives high weakly and reads back, so a short loses and an
//! external pull-down doesn't.

use wire_weaver::prelude::*;
pub use ww_gpio::{Level, Pull};
pub use ww_interned::Interned;
#[cfg(feature = "std")]
pub use ww_interned::InternedOwned;

/// Raw access to a board's digital points, see the [crate](crate) documentation.
#[ww_trait]
pub trait PinDiag {
    /// Every point a test may touch; a point's index in this list is its id in the calls below.
    const POINTS: RefVec<'i, Point<'i>>;

    /// Sets a point to an input with a pull, or to an output at a level. Takes the point from its normal function
    /// until [PinDiag::release_all]. Refused when the point can't do that at all, a function holds it, or the
    /// board's rules forbid it in the current state of the other points.
    #[errors(NoSuchPoint, NotCapable, Refused, Common(InUse))]
    fn set(point: u16, state: PointState) -> Result<(), PinDiagError<'i>>;

    /// Level on the point's pad, also while it drives: a point driving high that reads low is shorted to something
    /// stronger.
    #[errors(NoSuchPoint, NotCapable)]
    fn read(point: u16) -> Result<Level, PinDiagError<'i>>;

    /// Every point set since the last release goes back to the firmware's safe state and to its function.
    fn release_all();
}

/// One point a test may touch.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug, PartialEq, Eq)
)]
pub struct Point<'i> {
    /// As on the schematic or in the board's documentation: "GPIO2", "EX13".
    pub name: Interned<'i>,
    /// What kind of point, so a host can pick them together: "mcu", "chain".
    pub group: Interned<'i>,
    /// What may ever be done with it; the board's rules may refuse more, depending on the other points.
    pub caps: PointCaps,
}

/// What a point can do at all.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct PointCaps {
    /// [PinDiag::read] gives its pad level.
    pub read: bool,
    /// It may be an input with [Pull::Up].
    pub pull_up: bool,
    /// It may be an input with [Pull::Down].
    pub pull_down: bool,
    /// It may drive low.
    pub drive_low: bool,
    /// It may drive high.
    pub drive_high: bool,
    /// [Drive::Weak] is weaker than [Drive::Normal] on it.
    pub weak_drive: bool,
}

/// What [PinDiag::set] makes of a point.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum PointState {
    /// Input, with or without a pull resistor. `Pull::Custom` is the firmware's own setting.
    Input { pull: Pull },
    /// Output driving a level.
    Output { level: Level, drive: Drive },
}

/// Drive strength of an output.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum Drive {
    /// What the firmware uses for this point normally.
    Normal,
    /// The weakest drive the point has, for short tests.
    Weak,
}

/// Errors of [PinDiag].
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug, PartialEq, Eq),
    ww_repr = unib32,
    display
)]
pub enum PinDiagError<'i> {
    /// {0}
    #[common]
    Common(CommonError<'i>),
    /// No point with this id
    NoSuchPoint,
    /// The point can't do this at all (see its caps)
    NotCapable,
    /// The board's rules forbid this now: {rule}
    Refused { rule: Interned<'i> },
}

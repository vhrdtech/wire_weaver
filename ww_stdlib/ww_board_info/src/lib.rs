#![cfg_attr(not(feature = "std"), no_std)]
//! PCB and PCBA information (STD-24).
//!
//! One const item, [BoardInfo::BOARD], answers everything a host or a support case asks about the board: its name,
//! revision and **how the firmware knows that revision**, variant, BOM variant, serial number and build date.
//! [BoardInfo::REWORKS] is optional: which reworks of a bring-up board are applied, and [BoardInfo::set_reworks]
//! records a rework done on the bench (STD-98). [table] has what a firmware needs to answer both from its own table.
//!
//! A firmware never runs a board with a rework it doesn't know (STD-101): it refuses to boot, before any board
//! init, unless an allowance bound to its own image says otherwise. [guard] decides it, [image] carries the reworks
//! an image knows and where the board's record is inside the image itself, so a flasher can compare before it
//! writes, and [record] (feature `host`) reads and writes the record and the allowance through a boot ROM.

use core::fmt;
use wire_weaver::prelude::*;
pub use ww_date_time::DateTime;
use ww_error::CommonError;

pub mod guard;
pub mod image;
#[cfg(feature = "host")]
pub mod record;
pub mod table;

pub use guard::ImageId;

/// The PCB and its assembly.
///
/// Never fails: what the device doesn't know is an empty string or `None`.
#[ww_trait]
pub trait BoardInfo {
    /// Everything the device knows about its board, in one value.
    const BOARD: BoardSummary<'i>;

    /// Reworks and patches of this PCB revision (cuts, jumpers, swapped parts) and whether each is applied on this
    /// unit; for bring-up boards, where units of one revision differ. Every rework the firmware knows, in the order
    /// of the board's rework list, then the ones the unit's record has applied and the firmware doesn't know
    /// (`title: None`: the board runs because an allowance for this image covers them, STD-101). Left out by
    /// devices that don't keep the list.
    #[optional]
    #[since = "0.3.0"]
    const REWORKS: MaxLen<RefVec<'i, Rework<'i>>, MAX_REWORKS>;

    /// Record reworks done on this unit (or undone): every change or none. A number this firmware doesn't know is
    /// refused ([ReworkError::UnknownRework]), never stored. `program` false is a dry run: everything is checked, nothing
    /// written. The board runs on the record from its next boot: [BoardInfo::REWORKS], read once per connection,
    /// shows the board as it booted. A device that serves this serves [BoardInfo::REWORKS] too.
    #[optional]
    #[since = "0.3.0"]
    fn set_reworks(
        changes: MaxLen<RefVec<'i, ReworkState>, MAX_REWORKS>,
        program: bool,
    ) -> Result<ReworkChange, ReworkError<'i>>;
}

/// Most reworks a board lists, and most changes one [BoardInfo::set_reworks] carries.
pub const MAX_REWORKS: usize = 16;

/// Number of a rework in the board revision's rework list, `7` for "rework 7". Numbers are per board revision.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)
)]
pub struct ReworkId(pub u16);

impl fmt::Display for ReworkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// One rework of the board, see [BoardInfo::REWORKS].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq))]
pub struct Rework<'i> {
    /// Its number in the board revision's rework list. A number, not text or an `Interned` id: the unit's record
    /// keeps a bit per number, which any firmware and any support case can read.
    pub id: ReworkId,
    /// What it is, `cut trace to U3 pin 4`, from the firmware's table. `None`: the unit's record has this rework
    /// applied and this firmware doesn't know it (written by a newer one); the board runs only because an allowance
    /// for this image covers it, else the firmware would have refused to boot (STD-101).
    pub title: Option<MaxLen<&'i str, 48>>,
    /// Whether this unit has it applied.
    pub applied: bool,
}

/// One change for [BoardInfo::set_reworks].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Copy, Debug, PartialEq, Eq)
)]
pub struct ReworkState {
    pub id: ReworkId,
    /// Applied on this unit from now on, or undone.
    pub applied: bool,
}

/// What [BoardInfo::set_reworks] did, or would do in a dry run.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Copy, Debug, PartialEq, Eq),
    ww_repr = u4
)]
pub enum ReworkChange {
    /// The record already says exactly this: nothing to write.
    Unchanged,
    /// Dry run: this would be written.
    WouldWrite,
    /// Written; the board runs on it from the next boot, `REWORKS` says so from then on.
    Written,
}

/// Why [BoardInfo::set_reworks] refused; nothing was written.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug, PartialEq, Eq),
    display,
    ww_repr = unib32
)]
pub enum ReworkError<'i> {
    /// Common error: a storage fault (`Fault`), a stored record from a newer firmware (`WrongState`), ...
    #[display("{0}")]
    #[common]
    Common(CommonError<'i>),
    /// Unknown rework {id}: this firmware doesn't know it, update the firmware first
    ///
    /// `REWORKS` lists the ones it knows.
    UnknownRework { id: ReworkId },
    /// Rework {id} conflicts with rework {with}: undo one in the same call
    ///
    /// Two ways of rewiring the same lines, say.
    Conflict { id: ReworkId, with: ReworkId },
    /// A production record fixes this unit's reworks: a bench record would never be used
    ProductionRecord,
}

/// The PCB and its assembly, see [BoardInfo::BOARD].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq))]
pub struct BoardSummary<'i> {
    /// Board series, `B200`.
    pub name: MaxLen<&'i str, 32>,
    /// Hardware revision as printed on the board, `B`.
    pub revision: MaxLen<&'i str, 8>,
    /// How the firmware knows the revision.
    pub revision_source: RevisionSource,
    /// PCB assembly variant.
    pub variant: MaxLen<&'i str, 32>,
    /// PCB assembly BOM variant.
    pub bom_variant: MaxLen<&'i str, 32>,
    /// Serial number.
    pub serial: MaxLen<&'i str, 32>,
    /// Build date, if available.
    pub build_date: Option<DateTime>,
}

/// Where a board's revision comes from.
///
/// A unit running on an assumed revision is a different support case from one that read its own record.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Copy, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum RevisionSource {
    /// A record written in production (OTP, EEPROM, flash).
    Record,
    /// Resistor straps or a revision pin read at boot.
    Strap {
        /// The raw value read.
        value: u16,
    },
    /// Neither: the firmware assumes the revision it was built for.
    Assumed,
    /// A provisional record a bench or developer wrote in editable storage (flash), standing in for the production
    /// record until it is written; not a production record.
    #[since = "0.2.0"]
    Provisional,
}

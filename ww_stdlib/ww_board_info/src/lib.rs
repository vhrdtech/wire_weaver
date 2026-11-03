#![cfg_attr(not(feature = "std"), no_std)]
//! PCB and PCBA information (STD-24).
//!
//! One const item, [BoardInfo::BOARD], answers everything a host or a support case asks about the board: its name,
//! revision and **how the firmware knows that revision**, variant, BOM variant, serial number and build date.
//! [BoardInfo::REWORKS] is optional: which reworks of a bring-up board are applied, and [BoardInfo::set_reworks]
//! records a rework done on the bench (STD-98). [table] has what a firmware needs to answer both from its own table.
//!
//! A firmware never runs a board with a rework it doesn't know (STD-101): it boots guarded, reported by
//! [BoardInfo::GUARD], unless an allowance bound to its own image says otherwise
//! ([BoardInfo::allow_unknown_reworks]). [guard] decides it, [image] carries the reworks an image knows inside the
//! image itself, so a flasher can compare before it writes.

use core::fmt;
use wire_weaver::prelude::*;
pub use ww_date_time::DateTime;
use ww_error::CommonError;

pub mod guard;
pub mod image;
pub mod table;

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
    /// (`title: None`). Left out by devices that don't keep the list.
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

    /// Whether the board runs normally or guarded, and why (STD-101): a firmware boots guarded when the record has a
    /// rework applied it doesn't know, unless an allowance for this very image covers it. Guarded, it serves identity,
    /// info, updates and reading the record only, with every output in its safe state. Decided once at boot.
    #[optional]
    #[since = "0.3.0"]
    const GUARD: GuardSummary<'i>;

    /// Store an allowance: the image with this [ImageId] may run the board normally with these reworks it doesn't
    /// know. A flasher stores it for the image it is about to write, after the user agreed; on a guarded board it is
    /// stored for the running image, used from the next boot. It replaces any stored allowance, is void for every
    /// other image, and an image with the guard erases one that isn't its own at boot. `program` false is a dry run.
    /// Served in the guarded state.
    #[optional]
    #[since = "0.3.0"]
    fn allow_unknown_reworks(
        image: ImageId,
        reworks: MaxLen<RefVec<'i, ReworkId>, MAX_REWORKS>,
        program: bool,
    ) -> Result<ReworkChange, ReworkError<'i>>;

    /// Erase the stored allowance, if any. Flashers call it before writing an image that needs none. `program` false
    /// is a dry run. Served in the guarded state.
    #[optional]
    #[since = "0.3.0"]
    fn clear_unknown_reworks_allowance(program: bool) -> Result<ReworkChange, ReworkError<'i>>;
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
    /// applied and this firmware doesn't know it (written by a newer one); the board then boots guarded
    /// ([BoardInfo::GUARD]) until a firmware that knows it is flashed or the rework is allowed for this image.
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
    /// The board is guarded, rework {id} is unknown to this firmware: only reading the record works
    ///
    /// See [BoardInfo::GUARD]: flash a firmware that knows the rework, or allow it for this image.
    #[since = "0.3.0"]
    Guarded { id: ReworkId },
}

/// Which image an allowance is for: the first 8 bytes of its GNU build id (`ww_firmware_info::build_id`), the same
/// in the image file (`ww_board_info::image`) and in the running firmware. Not a security boundary: whoever can
/// store an allowance can also change the record.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Copy, Debug, PartialEq, Eq, Hash)
)]
pub struct ImageId(pub [u8; 8]);

impl ImageId {
    /// The id of the image with this build id; `None` when it is shorter than 8 bytes (no build-id note), or all
    /// zeros, which a stored allowance uses for "none".
    pub fn from_build_id(build_id: &[u8]) -> Option<ImageId> {
        let id: [u8; 8] = build_id.get(..8)?.try_into().ok()?;
        (id != [0; 8]).then_some(ImageId(id))
    }
}

impl fmt::Display for ImageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

/// How the board booted, see [BoardInfo::GUARD].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq))]
pub struct GuardSummary<'i> {
    pub state: GuardState,
    /// Reworks the record has applied and this firmware doesn't know; empty in [GuardState::Normal].
    pub unknown: MaxLen<RefVec<'i, ReworkId>, MAX_REWORKS>,
    /// The stored allowance, if any.
    pub allowance: Option<Allowance<'i>>,
}

/// See [GuardSummary].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Copy, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum GuardState {
    /// The firmware knows every rework applied: the board runs normally.
    Normal,
    /// A rework applied is unknown and no allowance for this image covers it: identity, info, updates and reading the
    /// record only, outputs in their safe state.
    Guarded,
    /// Unknown reworks applied, run normally anyway: the allowance for this image covers them.
    Allowed,
}

/// An allowance as stored, see [BoardInfo::allow_unknown_reworks].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq))]
pub struct Allowance<'i> {
    /// The image it is for.
    pub image: ImageId,
    /// The unknown reworks it lets that image run with.
    pub reworks: MaxLen<RefVec<'i, ReworkId>, MAX_REWORKS>,
    /// It is for the running image (else: stored for an image about to be flashed).
    pub for_running_image: bool,
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

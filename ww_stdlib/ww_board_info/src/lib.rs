#![cfg_attr(not(feature = "std"), no_std)]
//! PCB and PCBA information (STD-24).
//!
//! One const item, [BoardInfo::BOARD], answers everything a host or a support case asks about the board: its name,
//! revision and **how the firmware knows that revision**, variant, BOM variant, serial number and build date.
//! [BoardInfo::REWORKS] is optional: which reworks of a bring-up board are applied.

use wire_weaver::prelude::*;
pub use ww_date_time::DateTime;

/// The PCB and its assembly.
///
/// Never fails: what the device doesn't know is an empty string or `None`.
#[ww_trait]
pub trait BoardInfo {
    /// Everything the device knows about its board, in one value.
    const BOARD: BoardSummary<'i>;

    /// Reworks and patches of this PCB revision (cuts, jumpers, swapped parts) and whether each is applied on this
    /// unit; for bring-up boards, where units of one revision differ. Left out by devices that don't keep the list.
    #[optional]
    #[since = "0.3.0"]
    const REWORKS: MaxLen<RefVec<'i, Rework<'i>>, MAX_REWORKS>;
}

/// Most reworks a board lists.
pub const MAX_REWORKS: usize = 16;

/// One rework of the board, see [BoardInfo::REWORKS].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq))]
pub struct Rework<'i> {
    /// Stable short id, `rework-5`: the name used in the rework list of the board's documentation. Text, not an
    /// `Interned` id: an id of the firmware's string table changes with every build and needs the ELF to read, while
    /// a rework is a fact about the hardware that a support case must read from any unit.
    pub id: MaxLen<&'i str, 16>,
    /// What it is, `cut trace to U3 pin 4`.
    pub title: MaxLen<&'i str, 48>,
    /// Whether this unit has it applied.
    pub applied: bool,
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

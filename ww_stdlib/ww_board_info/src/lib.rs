#![cfg_attr(not(feature = "std"), no_std)]
//! PCB and PCBA information (STD-24).
//!
//! One const item, [BoardInfo::BOARD], answers everything a host or a support case asks about the board: its name,
//! revision and **how the firmware knows that revision**, variant, BOM variant, serial number and build date.

use wire_weaver::prelude::*;
pub use ww_date_time::DateTime;

/// The PCB and its assembly.
///
/// Never fails: what the device doesn't know is an empty string or `None`.
#[ww_trait]
pub trait BoardInfo {
    /// Everything the device knows about its board, in one value.
    const BOARD: BoardSummary<'i>;
}

/// The PCB and its assembly, see [BoardInfo::BOARD].
#[derive_shrink_wrap(
    owned(feature = "std"),
    derive(Clone, Debug, PartialEq, Eq)
)]
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
}

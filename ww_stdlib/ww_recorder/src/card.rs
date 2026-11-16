use ww_api::prelude::*;
use ww_si::prefix::Kibi;
use ww_si::units::BytePerSecond;
use ww_si::{Bytes, Fixed, Mebibytes, Millis, Secs};

use crate::UnknownableBytes;

/// Longest name in the status and card facts (card product name), in bytes.
pub const MAX_NAME: usize = 48;

/// One-use token of a [FormatOffer], see [Recorder::format].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    transparent
)]
pub struct FormatToken(pub u32);

/// Serial number of the card (CID), see [CardFacts::serial].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash),
    transparent
)]
pub struct CardSerial(pub u32);

/// The card, see [Recorder::card].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct CardFacts<'i> {
    pub kind: CardKind,
    /// Capacity the card reports.
    pub capacity: Bytes<u64>,
    /// Product name (CID, e.g. `SD64G`), empty if the medium has none.
    pub product: MaxLen<&'i str, MAX_NAME>,
    /// Manufacturer id (CID).
    pub manufacturer: u8,
    /// Serial number (CID).
    pub serial: CardSerial,
    /// Allocation unit (SD Status `AU_SIZE`, the larger UHS/video one where given), unknown if the card reports
    /// none: the log then uses 4 MB.
    pub au: UnknownableBytes,
    /// Recording unit of the card's speed class (16 KB and up), unknown if not reported.
    pub recording_unit: UnknownableBytes,
    /// Speed class (`SPEED_CLASS`: 2, 4, 6, 10), 0 unknown.
    pub speed_class: u8,
    /// UHS speed grade (1, 3), 0 unknown.
    pub uhs_grade: u8,
    /// Video speed class (6, 10, 30, 60, 90), 0 unknown.
    pub video_class: u8,
    /// What is on the card.
    pub content: CardContent,
    /// Result of the last card check, `None` before one.
    pub check: Option<CardCheck>,
}

/// Kind of medium, see [CardFacts::kind].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum CardKind {
    Sdsc,
    Sdhc,
    Sdxc,
    /// Soldered or socketed eMMC.
    Emmc,
    /// SPI or QSPI NOR flash.
    NorFlash,
}

/// What is on a card, see [CardFacts::content].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum CardContent {
    /// Nothing recognised (blank, or a filesystem the device doesn't read).
    Unrecognised,
    /// A volume with files and no ring.
    OtherFiles,
    /// A ring this device mounts.
    Ring,
    /// A ring file that is not where the device put it, see [State::Unbound].
    UnboundRing,
}

/// Result of a card check, see [CardFacts::check]. Rates the card against [Settings] and the data rate configured.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct CardCheck {
    /// Capacity really there: a fake card wraps around when written past its true size.
    pub aliasing_ok: bool,
    /// Sustained sequential write speed.
    pub write_rate: Fixed<BytePerSecond, Kibi, u32>,
    /// Longest single write seen.
    pub worst_stall: Millis<u32>,
    /// Flush size that wrote fastest on this card (0: not measured): the device's default for [Settings::flush].
    pub best_flush: Bytes<u32>,
    pub rating: Rating,
}

/// How the card does against the configured data rate, see [CardCheck::rating].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum Rating {
    /// Keeps up with the configured rate with the RAM buffer's margin for its stalls.
    Good,
    /// Keeps up on average, but its worst stall is longer than the buffer holds: drops are possible.
    Marginal,
    /// Slower than the configured rate: drops are certain.
    TooSlow,
    /// Not checked.
    Unchecked,
}

/// What a format starts from, see [Recorder::prepare_format].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum FormatAction {
    /// Erase the card: new filesystem, new ring, new read-write region.
    Format,
    /// Create the ring file again after [State::Unbound], keeping the files of the read-write region.
    RecreateRing,
}

/// What a format would do, the answer of [Recorder::prepare_format].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct FormatOffer<'i> {
    /// Pass to [Recorder::format].
    pub token: FormatToken,
    /// How long the token stays valid.
    pub valid_for: Secs<u16>,
    pub action: FormatAction,
    /// The card, with the result of its last check if it had one.
    pub card: CardFacts<'i>,
    /// What the second step erases: the whole card for [FormatAction::Format].
    pub erases: Bytes<u64>,
    /// Whether the card holds a ring with data, and files in its read-write region. A UI warns on these.
    pub has_ring_data: bool,
    pub has_rw_files: bool,
    /// Read-write region size: the default and the limits.
    pub rw_mb_default: Mebibytes<u32>,
    pub rw_mb_min: Mebibytes<u32>,
    pub rw_mb_max: Mebibytes<u32>,
    /// What the ring gets with the default region.
    pub ring: Bytes<u64>,
}

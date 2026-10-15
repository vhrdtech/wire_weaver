#![cfg_attr(not(feature = "std"), no_std)]
//! Unique ids of a device (STD-24).
//!
//! A unit can carry several ids: a chip's factory-programmed unique id, a MAC address, an EUI-64 of a radio. They
//! are reported as one list, [Uid::IDS]. The first one is the **primary id**: the one a device puts into its USB
//! serial number string (as lower-case hex, no separators) and a host shows in `ww list`. A host that lost a device
//! on a reboot into a ROM bootloader sharing the same id (RP2350 BOOTSEL, STM32 DFU) finds the unit again by it.

use wire_weaver::prelude::*;

/// The unique ids of this unit.
///
/// The list is never empty on a device that has any id at all; the first entry is the primary id, see the
/// [crate](crate) documentation.
#[ww_trait]
pub trait Uid {
    /// All ids of this unit, the primary one first. At most 4.
    const IDS: MaxLen<RefVec<'i, UniqueId<'i>>, 4>;
}

/// One unique id of a unit, where it comes from and what kind of number it is.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug, PartialEq, Eq)
)]
pub struct UniqueId<'i> {
    /// What the bytes of `value` are.
    pub kind: UidKind<'i>,
    /// Who provides the id.
    pub source: UidSource<'i>,
    /// The id, up to 16 bytes: 6 for a MAC, 8 for an EUI-64, 16 for a UUID, a chip's own length otherwise.
    pub value: MaxLen<RefVec<'i, u8>, 16>,
}

/// What an id's bytes are.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum UidKind<'i> {
    /// A number unique to this chip, programmed by its manufacturer.
    ChipUnique,
    /// A 48-bit MAC address.
    Mac48,
    /// A 64-bit extended unique identifier.
    Eui64,
    /// A 128-bit UUID.
    Uuid,
    /// Anything else, named.
    Other(MaxLen<&'i str, 16>),
}

/// Who provides an id.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum UidSource<'i> {
    /// Inside the main chip (factory-programmed ID registers).
    InternalIc,
    /// A separate chip on the board (an EEPROM with a MAC, a 1-Wire ID chip).
    ExternalIc,
    /// Written by firmware: stored in flash or generated at the first start.
    Firmware,
    /// Read from resistor straps or pins.
    HardwareStraps,
    /// Anything else, named.
    Other(MaxLen<&'i str, 16>),
}

//! Low-level access to memory or registers over WireWeaver: read and write words at addresses, and optionally watch
//! addresses for changes.
//!
//! | Trait      | Address | Word  | For                                                       |
//! |------------|---------|-------|-----------------------------------------------------------|
//! | `Memory8`  | `u8`    | `u8`  | Register maps of small chips (I2C/SPI sensors, PMICs).    |
//! | `Memory32` | `u32`   | `u32` | Memory and peripheral registers of a 32-bit MCU.          |
//!
//! Every read, write and watch of a trait uses the same word type, and an address counts words of that type.
//! Watching is `#[optional(watches)]`: a device without it leaves `watch`, `watching` and `changed` out
//! (`omit = ["mem.watches"]`), and a host calling them gets `Unimplemented`.
//!
//! Errors are [MemoryError]: the framework's `CommonError` as [MemoryError::Common], plus what is specific to
//! memory access. Each method lists the variants it returns (`#[errors]`).
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

/// Memory or registers with 8-bit addresses and 8-bit words.
#[ww_trait]
pub trait Memory8 {
    /// Reads the word at `addr`. With `allow_side_effects` false, an address whose read changes the device's state
    /// (e.g. a status register cleared on read) isn't read and answers `Common(Refused)`.
    #[errors(WrongAddress, Common(Refused))]
    fn read(addr: u8, allow_side_effects: bool) -> Result<u8, MemoryError<'i>>;
    /// Writes the word at `addr`; `Common(Refused)` for a read-only address.
    #[errors(WrongAddress, Common(Refused))]
    fn write(addr: u8, value: u8) -> Result<(), MemoryError<'i>>;

    /// Reads `len` words starting at `addr`. With `allow_side_effects` false, addresses whose read changes the
    /// device's state are skipped.
    #[errors(WrongAddress, Common(Refused))]
    fn read_many(
        addr: u8,
        len: u8,
        allow_side_effects: bool,
    ) -> Result<RefVec<'i, u8>, MemoryError<'i>>;
    /// Writes `data` to consecutive words starting at `addr`.
    #[errors(WrongAddress, Common(Refused))]
    fn write_many(addr: u8, data: RefVec<'i, u8>) -> Result<(), MemoryError<'i>>;

    /// Starts watching the word at `addr`: its changes come on `changed`.
    #[optional(watches)]
    #[errors(WrongAddress, OutOfWatchSlots)]
    fn watch(addr: u8) -> Result<(), MemoryError<'i>>;
    /// Watched addresses.
    #[optional(watches)]
    fn watching() -> RefVec<'i, u8>;
    /// Changes of the watched words: `(address, new value)`.
    #[optional(watches)]
    stream!(changed: (u8, u8));

    /// Valid addresses, first and last.
    property!(ro address_range: (u8, u8));
    /// Access rights for this memory region.
    property!(ro access: Access);
}

/// Memory or registers with 32-bit addresses and 32-bit words.
#[ww_trait]
pub trait Memory32 {
    /// Reads the word at `addr`. With `allow_side_effects` false, an address whose read changes the device's state
    /// (e.g. a status register cleared on read) isn't read and answers `Common(Refused)`.
    #[errors(WrongAddress, Common(Refused))]
    fn read(addr: u32, allow_side_effects: bool) -> Result<u32, MemoryError<'i>>;
    /// Writes the word at `addr`; `Common(Refused)` for a read-only address.
    #[errors(WrongAddress, Common(Refused))]
    fn write(addr: u32, value: u32) -> Result<(), MemoryError<'i>>;

    /// Reads `len` words starting at `addr`. With `allow_side_effects` false, addresses whose read changes the
    /// device's state are skipped.
    #[errors(WrongAddress, Common(Refused))]
    fn read_many(
        addr: u32,
        len: u32,
        allow_side_effects: bool,
    ) -> Result<RefVec<'i, u32>, MemoryError<'i>>;
    /// Writes `data` to consecutive words starting at `addr`.
    #[errors(WrongAddress, Common(Refused))]
    fn write_many(addr: u32, data: RefVec<'i, u32>) -> Result<(), MemoryError<'i>>;

    /// Starts watching the word at `addr`: its changes come on `changed`.
    #[optional(watches)]
    #[errors(WrongAddress, OutOfWatchSlots)]
    fn watch(addr: u32) -> Result<(), MemoryError<'i>>;
    /// Watched addresses.
    #[optional(watches)]
    fn watching() -> RefVec<'i, u32>;
    /// Changes of the watched words: `(address, new value)`.
    #[optional(watches)]
    stream!(changed: (u32, u32));

    /// Valid addresses, first and last.
    property!(ro address_range: (u32, u32));
    /// Access rights for this memory region.
    property!(ro access: Access);
}

/// Access rights of a memory region.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug),
    self_describing,
    ww_repr = u2
)]
pub enum Access {
    WriteOnly,
    ReadOnly,
    ReadWrite,
    Custom(Nibble),
}

/// Errors of `Memory8` and `Memory32`: the common ones first, then what is specific to memory access. Each method
/// lists the variants it returns.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Eq, Clone),
    ww_repr = unib32,
    display
)]
pub enum MemoryError<'i> {
    /// {0}
    #[common]
    Common(CommonError<'i>),
    /// An address outside `address_range`, or a range that runs past its end.
    WrongAddress,
    /// All watch slots are taken.
    OutOfWatchSlots,
}

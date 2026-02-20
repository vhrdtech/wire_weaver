#![no_std]

use wire_weaver::prelude::*;

/// Memory or register low-level access interface with 8-bit addressing.
#[ww_trait]
pub trait Memory8 {
    /// Read one byte at addr.
    /// if allow_side_effects is false, addresses that change system state on read must be skipped (e.g., status registers).
    fn read(addr: u8, allow_side_effects: bool) -> Result<u8, Error>;
    /// Write one byte at addr.
    fn write(addr: u8, value: u8) -> Result<(), Error>;

    /// Read array of bytes starting from addr.
    /// if allow_side_effects is false, addresses that change system state on read must be skipped (e.g., status registers).
    fn read_many(addr: u8, len: u8, allow_side_effects: bool) -> Result<RefVec<'i, u8>, Error>;
    /// Write array of bytes starting from addr.
    fn write_many(addr: u8, data: RefVec<'i, u8>) -> Result<(), Error>;

    /// Start watching memory at address for changes. Optional.
    fn watch(addr: u8) -> Result<(), Error>;
    /// Returns a list of watched addresses. Optional.
    fn watching() -> Result<RefVec<'i, u8>, Error>;
    /// Stream of changes to memory at watched addresses. Optional.
    stream!(changed: (u8, u8));

    /// Valid address range.
    property!(ro address_range: (u8, u8));
    /// Access rights for this memory region.
    property!(ro access: Access);
}

/// Memory or register low-level access interface with 32-bit addressing.
#[ww_trait]
pub trait Memory32 {
    /// Read one byte at addr.
    /// if allow_side_effects is false, addresses that change system state on read must be skipped (e.g., status registers).
    fn read(addr: u32, allow_side_effects: bool) -> Result<u8, Error>;
    /// Write one byte at addr.
    fn write(addr: u32, value: u32) -> Result<(), Error>;

    /// Read array of bytes starting from addr.
    /// if allow_side_effects is false, addresses that change system state on read must be skipped (e.g., status registers).
    fn read_many(addr: u32, len: u32, allow_side_effects: bool) -> Result<RefVec<'i, u32>, Error>;
    /// Write array of bytes starting from addr.
    fn write_many(addr: u32, data: RefVec<'i, u32>) -> Result<(), Error>;

    /// Start watching memory at address for changes. Optional.
    fn watch(addr: u32) -> Result<(), Error>;
    /// Returns a list of watched addresses. Optional.
    fn watching() -> Result<RefVec<'i, u32>, Error>;
    /// Stream of changes to memory at watched addresses. Optional.
    stream!(changed: (u32, u32));

    /// Valid address range.
    property!(ro address_range: (u32, u32));
    /// Access rights for this memory region.
    property!(ro access: Access);
}

#[derive_shrink_wrap]
#[ww_repr(u2)]
#[derive(Clone, Debug)]
#[sized]
pub enum Access {
    WriteOnly,
    ReadOnly,
    ReadWrite,
    Custom(Nibble)
}

#[derive_shrink_wrap]
#[ww_repr(u4)]
#[derive(Clone, Debug)]
#[sized]
pub enum Error {
    WrongAddress,
    WatchNotSupported,
    OutOfWatchSlots,
}
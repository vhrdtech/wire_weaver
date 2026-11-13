#![cfg_attr(not(feature = "std"), no_std)]
//! A name and notes the user gives a unit (STD-24), shown by `ww list`.
//!
//! # Persistence
//!
//! [UserInfo::CAPABILITIES] says whether a written value survives a power cycle (`persistent`). When it does, a write
//! is saved **before the call answers**: an `Ok` means the value survives a power cut. A device writes its storage
//! only when the value actually changed (flash wear), so a host may write the same value again freely. A device
//! that cannot save returns `Fault`, one that cannot interrupt a flash operation `Busy`.

use wire_weaver::prelude::*;
use ww_error::CommonError;
pub use ww_si::{Bytes, Range, units::Byte};

/// Longest name any device keeps, in bytes.
pub const MAX_NAME_LEN: usize = 32;
/// Longest notes any device keeps, in bytes.
pub const MAX_NOTES_LEN: usize = 256;

/// A device's name length cap, 0 to [MAX_NAME_LEN] bytes.
pub type NameMax = Bytes<u8, Range<0, { MAX_NAME_LEN as i64 }>>;
/// A device's notes length cap, 0 to [MAX_NOTES_LEN] bytes (0 when notes are left out).
pub type NotesMax = Bytes<u16, Range<0, { MAX_NOTES_LEN as i64 }>>;

/// A name and notes the user gives this unit.
///
/// # Errors
///
/// Writes return [UserInfoError]: `TooLong` for a value above what [UserInfo::CAPABILITIES] says, `Common(Busy)`
/// while storage can't be touched, `Common(Fault)` when saving failed.
#[ww_trait]
pub trait UserInfo {
    /// Lengths this device keeps and whether a written value survives a power cycle.
    const CAPABILITIES: UserInfoCaps;

    /// Short name of the unit, shown by `ww list`.
    #[errors(TooLong, Common(Busy, Fault))]
    property!(rw name: MaxLen<&'i str, MAX_NAME_LEN>, UserInfoError<'i>);

    /// Free-form notes about the unit. A device with no room for them leaves this out.
    #[optional]
    #[errors(TooLong, Common(Busy, Fault))]
    property!(rw notes: MaxLen<&'i str, MAX_NOTES_LEN>, UserInfoError<'i>);
}

/// What a device keeps of the user's name and notes.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Copy, Debug, PartialEq, Eq)
)]
pub struct UserInfoCaps {
    /// Longest name the device keeps, in bytes (at most [MAX_NAME_LEN]).
    pub name_max: NameMax,
    /// Longest notes the device keeps, in bytes (at most [MAX_NOTES_LEN], 0 when notes are left out).
    pub notes_max: NotesMax,
    /// A written value survives a power cycle.
    pub persistent: bool,
}

/// Errors of writing the name or notes.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug, PartialEq, Eq),
    display,
    ww_repr = unib32
)]
pub enum UserInfoError<'i> {
    /// Common error
    #[display("{0}")]
    #[common]
    Common(CommonError<'i>),
    /// Longer than this device keeps ({max} bytes)
    TooLong {
        /// What the device keeps, from `CAPABILITIES`.
        max: Bytes<u16>,
    },
}

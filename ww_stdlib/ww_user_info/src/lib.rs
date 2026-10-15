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

/// Longest name any device keeps, in bytes.
pub const MAX_NAME_LEN: usize = 32;
/// Longest notes any device keeps, in bytes.
pub const MAX_NOTES_LEN: usize = 256;

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
    property!(rw name: MaxLen<&'i str, MAX_NAME_LEN>, UserInfoError<'i>);

    /// Free-form notes about the unit. A device with no room for them leaves this out.
    #[optional]
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
    /// Longest name the device keeps, in bytes (at most 32).
    pub name_max: u16,
    /// Longest notes the device keeps, in bytes (at most 256, 0 when notes are left out).
    pub notes_max: u16,
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
        max: u16,
    },
}

#![cfg_attr(not(feature = "std"), no_std)]
//! Errors every WireWeaver API can return (STD-16).
//!
//! [CommonError] is not part of any API's error enum: a handler returns it beside the API's own error and the
//! protocol carries it on its own (`ww_client_server::ErrorKind::Common`), so every client sees the same type
//! whatever the API. An API's error enum keeps only what is specific to it:
//!
//! ```
//! use shrink_wrap::prelude::*;
//! use ww_error::CommonError;
//!
//! #[derive_shrink_wrap(derive(Debug, PartialEq, Eq, Clone), ww_repr = unib32)]
//! pub enum I2cError {
//!     /// The address or a data byte was not acknowledged.
//!     Nack,
//! }
//!
//! // a helper returns `CommonError`; a handler's `?` turns it into the runtime's `Error<I2cError>`
//! fn check_power(on: bool) -> Result<(), CommonError<'static>> {
//!     if on { Ok(()) } else { Err(CommonError::NoPower { rail: MaxLen::new_unchecked("VIO") }) }
//! }
//! let e = check_power(false).unwrap_err();
//! assert_eq!(e.to_string(), "No power on VIO");
//! assert_eq!(e.name(), "NoPower");
//! ```
//!
//! Strings are bounded and may be empty: a device that saves flash sends `""`.

use core::fmt;
use shrink_wrap::prelude::*;
use ww_si::{Index, units::Argument};

/// Errors every item of every API can answer with, carried by the protocol beside the API's own error type.
///
/// `Unsupported` means the device knows the call and can't do what was asked; a call that has no implementation
/// at all is the framework's `Unimplemented` instead.
#[derive_shrink_wrap(
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    ww_repr = unib32
)]
pub enum CommonError<'i> {
    /// Not supported by this device (a mode, a range, a feature of the call).
    Unsupported,
    /// Argument number {arg} is invalid: {reason}.
    InvalidArgument {
        /// Position of the argument, starting at 0.
        arg: Index<Argument, u8>,
        reason: MaxLen<&'i str, 64>,
    },
    /// In use by {by}.
    InUse { by: MaxLen<&'i str, 32> },
    /// Busy with another operation, try again later.
    Busy,
    /// Not possible in the current state: {reason}.
    WrongState { reason: MaxLen<&'i str, 64> },
    /// No power on {rail}.
    NoPower { rail: MaxLen<&'i str, 16> },
    /// Refused: {reason}.
    Refused { reason: MaxLen<&'i str, 64> },
    /// Timed out.
    Timeout,
    /// Hardware fault {code:#x}.
    Fault { code: u32 },
}

impl CommonError<'_> {
    /// Every variant's name and its first doc line, for tools: `#[errors(CommonError(Busy, InUse))]` lists and
    /// their docs, `ww api tree`.
    pub const DOCS: &[(&str, &str)] = &[
        (
            "Unsupported",
            "Not supported by this device (a mode, a range, a feature of the call)",
        ),
        (
            "InvalidArgument",
            "Argument number {arg} is invalid: {reason}",
        ),
        ("InUse", "In use by {by}"),
        ("Busy", "Busy with another operation, try again later"),
        ("WrongState", "Not possible in the current state: {reason}"),
        ("NoPower", "No power on {rail}"),
        ("Refused", "Refused: {reason}"),
        ("Timeout", "Timed out"),
        ("Fault", "Hardware fault {code:#x}"),
    ];

    /// The variant's name, as in [CommonError::DOCS]; `Unknown` for a variant of a newer version.
    pub fn name(&self) -> &'static str {
        name_of(self.discriminant())
    }
}

/// [CommonError::name] by discriminant: the variants are numbered in the order of [CommonError::DOCS].
fn name_of(discriminant: u32) -> &'static str {
    CommonError::DOCS
        .get(discriminant as usize)
        .map_or("Unknown", |(name, _)| name)
}

#[cfg(feature = "std")]
impl CommonErrorOwned {
    /// The variant's name, as in [CommonError::DOCS]; `Unknown` for a variant of a newer version.
    pub fn name(&self) -> &'static str {
        name_of(self.discriminant())
    }
}

#[cfg(feature = "std")]
impl CommonError<'_> {
    /// The same error owning its strings.
    pub fn make_owned(&self) -> CommonErrorOwned {
        fn s<const N: usize>(s: &MaxLen<&str, N>) -> MaxLen<String, N> {
            MaxLen::new_unchecked((**s).to_owned())
        }
        match self {
            CommonError::Unsupported => CommonErrorOwned::Unsupported,
            CommonError::InvalidArgument { arg, reason } => CommonErrorOwned::InvalidArgument {
                arg: *arg,
                reason: s(reason),
            },
            CommonError::InUse { by } => CommonErrorOwned::InUse { by: s(by) },
            CommonError::Busy => CommonErrorOwned::Busy,
            CommonError::WrongState { reason } => {
                CommonErrorOwned::WrongState { reason: s(reason) }
            }
            CommonError::NoPower { rail } => CommonErrorOwned::NoPower { rail: s(rail) },
            CommonError::Refused { reason } => CommonErrorOwned::Refused { reason: s(reason) },
            CommonError::Timeout => CommonErrorOwned::Timeout,
            CommonError::Fault { code } => CommonErrorOwned::Fault { code: *code },
            CommonError::Unknown(n) => CommonErrorOwned::Unknown(*n),
        }
    }
}

// Written by hand, not with the derive's `display` (DER-12): a part with an empty string is left out ("Refused", not
// "Refused: "), which a doc-line template can't say. Same convention otherwise: no trailing period.
/// `text`, followed by `": {reason}"` when `reason` isn't empty.
fn with_reason(f: &mut fmt::Formatter<'_>, text: &str, reason: &str) -> fmt::Result {
    f.write_str(text)?;
    if !reason.is_empty() {
        write!(f, ": {reason}")?;
    }
    Ok(())
}

macro_rules! impl_display {
    ($ty:ty) => {
        impl fmt::Display for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self {
                    Self::Unsupported => f.write_str("Not supported by this device"),
                    Self::InvalidArgument { arg, reason } => {
                        write!(f, "Argument number {arg} is invalid")?;
                        with_reason(f, "", reason)
                    }
                    Self::InUse { by } if by.is_empty() => f.write_str("In use"),
                    Self::InUse { by } => write!(f, "In use by {}", &**by),
                    Self::Busy => f.write_str("Busy with another operation, try again later"),
                    Self::WrongState { reason } => {
                        with_reason(f, "Not possible in the current state", reason)
                    }
                    Self::NoPower { rail } if rail.is_empty() => f.write_str("No power"),
                    Self::NoPower { rail } => write!(f, "No power on {}", &**rail),
                    Self::Refused { reason } => with_reason(f, "Refused", reason),
                    Self::Timeout => f.write_str("Timed out"),
                    Self::Fault { code } => write!(f, "Hardware fault {code:#x}"),
                    Self::Unknown(n) => write!(f, "Common error {n} from a newer version"),
                }
            }
        }

        impl core::error::Error for $ty {}
    };
}

impl_display!(CommonError<'_>);
#[cfg(feature = "std")]
impl_display!(CommonErrorOwned);

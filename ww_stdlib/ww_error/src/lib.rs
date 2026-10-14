#![cfg_attr(not(feature = "std"), no_std)]
//! Errors every WireWeaver API can return (STD-16).
//!
//! An API's error enum puts [CommonError] first, marked `#[common]`, and keeps its own variants for what is
//! specific to it:
//!
//! ```
//! use shrink_wrap::prelude::*;
//! use ww_error::{CommonError, HasCommon};
//!
//! #[derive_shrink_wrap(derive(Debug, PartialEq, Eq, Clone), ww_repr = unib32)]
//! pub enum I2cError<'i> {
//!     #[common]
//!     Common(CommonError<'i>),
//!     /// The address or a data byte was not acknowledged.
//!     Nack,
//! }
//!
//! // `#[common]` adds `From<CommonError>`: helpers return `CommonError` and `?` into any crate's error
//! fn check_power(on: bool) -> Result<(), CommonError<'static>> {
//!     if on { Ok(()) } else { Err(CommonError::NoPower { rail: MaxLen::new_unchecked("VIO") }) }
//! }
//! fn write() -> Result<(), I2cError<'static>> {
//!     check_power(false)?;
//!     Ok(())
//! }
//!
//! // ... and `HasCommon`: generic code finds the common part of any crate's error
//! let e = write().unwrap_err();
//! assert!(matches!(e.common(), Some(CommonError::NoPower { .. })));
//! assert_eq!(e.common().unwrap().to_string(), "No power on VIO");
//! assert_eq!(I2cError::Nack.common(), None);
//! ```
//!
//! Tools (ww CLI, GUI, Python) recognise `ww_error::CommonError` by its type in the API model and show it the same
//! for every API. Strings are bounded and may be empty: a device that saves flash sends `""`.

// `#[common]` in derived enums names this crate `::ww_error`, here too
extern crate self as ww_error;

use core::fmt;
use shrink_wrap::prelude::*;

/// Errors every crate can return, recognised by tools by this type.
///
/// Put first in an API's error enum as `#[common] Common(CommonError<'i>)`. `Unsupported` means the device knows
/// the call and can't do what was asked; a call that has no implementation at all is the framework's
/// `Unimplemented` instead.
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
        arg: u8,
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

/// An error enum with a `#[common]` variant: generic code gets the [CommonError] out of any crate's error.
///
/// Implemented by `#[derive_shrink_wrap]` for an enum with a `#[common] Common(CommonError<'i>)` variant, with
/// `Common = CommonError<'i>` for the borrowed type and `Common = CommonErrorOwned` for the owned one, and by
/// `CommonError` itself.
pub trait HasCommon {
    type Common;

    /// The common error, `None` for a crate-specific variant.
    fn common(&self) -> Option<&Self::Common>;
}

impl<'i> HasCommon for CommonError<'i> {
    type Common = CommonError<'i>;

    fn common(&self) -> Option<&Self::Common> {
        Some(self)
    }
}

#[cfg(feature = "std")]
impl HasCommon for CommonErrorOwned {
    type Common = CommonErrorOwned;

    fn common(&self) -> Option<&Self::Common> {
        Some(self)
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

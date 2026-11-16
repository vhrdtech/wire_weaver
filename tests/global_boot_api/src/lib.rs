//! Two global traits for `tests/cross_version` (CLT-10, API-20, SRV-53): [Boot], implemented by the simulated
//! firmware, and [Missing], registered but never implemented, to test the "no such trait" path. Standing in for a
//! real `ww_stdlib` crate (R7): only the global-path shape matters here, not any real API's behavior.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

/// Reboots a unit between its app and its updater; implemented by the simulated old firmware.
#[ww_trait]
pub trait Boot {
    /// Where the device stands between two updates.
    fn boot_state() -> BootState;
    /// The running image works: keep it.
    fn confirm() -> Result<(), Error>;
    /// Give up the running trial image.
    fn revert() -> Result<(), Error>;
    /// Reset into the app or into the updater.
    fn reboot(into: BootTarget) -> Result<(), Error>;
}

/// Registered but never implemented by anything in `tests/cross_version`: calls into it always answer
/// `TraitNotFound`.
#[ww_trait]
pub trait Missing {
    fn ping() -> u8;
}

/// Where the device stands between two updates, see [Boot::boot_state].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = nib
)]
pub enum BootState {
    Confirmed,
    Trial,
}

/// Where [Boot::reboot] goes.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = nib
)]
pub enum BootTarget {
    App,
    Updater,
}

/// Why a [Boot] call failed.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = nib
)]
pub enum Error {
    Unsupported,
}

//! API of the `serve_trait` tests (SRV-49, `tests/serve_trait`), its local trait `Served` (CLT-57) and the same
//! trait without `Send`, `LocalServed` (SRV-73).
use ww_api::prelude::*;

#[ww_api_root]
#[effect(edit)]
pub trait ServeTrait {
    /// Answers `gate` once the test opened it
    fn wait(gate: Gate) -> Gate;
    /// Fails the way `how` says, or answers `Gate(0)`
    fn fail(how: How) -> Result<Gate, ServeError>;
    /// The gate a write names
    property!(rw mode: Gate);
    /// `count` ticks, `Err(ServeError::Refused)` for 0
    #[stream(Tick)]
    fn ticks(count: Tick) -> Result<Tick, ServeError>;
}

/// Which gate a call waits for.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq)
)]
pub struct Gate(pub u8);

/// A stream's item, its number.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq)
)]
pub struct Tick(pub u32);

/// How `fail` fails.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, Clone, Copy, PartialEq, Eq), ww_repr = u4)]
pub enum How {
    Not,
    Api,
    Denied,
    Internal,
    Busy,
    Unimplemented,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq), ww_repr = u4)]
pub enum ServeError {
    Refused,
}

ww_api::api_error!(ServeError);

impl core::fmt::Display for ServeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ServeError::Refused => write!(f, "refused"),
            ServeError::Unknown(n) => write!(f, "unknown error {n}"),
        }
    }
}

impl core::error::Error for ServeError {}

/// `Served`, the API as an async trait.
#[cfg(feature = "local")]
mod local {
    wire_weaver::ww_codegen!(crate::ServeTrait, local_trait = "Served",);
}
#[cfg(feature = "local")]
pub use local::{Served, ServedDyn};

/// `LocalServed`, the API as an async trait without `Send`.
#[cfg(feature = "local")]
mod local_not_send {
    wire_weaver::ww_codegen!(
        crate::ServeTrait,
        local_trait = "LocalServed",
        local_trait_send = false,
    );
}
#[cfg(feature = "local")]
pub use local_not_send::{LocalServed, LocalServedDyn};

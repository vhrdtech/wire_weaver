//! A global trait for `tests/dispatch_golden` (TEST-31, API-20): a const next to a writable property, reached by
//! `GlobalFull` and `GlobalCompact`. Standing in for a real `ww_stdlib` crate (R7): only the global-path shape
//! matters here, not any real API's behavior.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_trait]
pub trait GlobalDemo {
    /// Read on a global path, next to the property below.
    const CAPABILITIES: u8;
    /// Written and read on a global path.
    property!(rw name: MaxLen<&'i str, 32>);
}

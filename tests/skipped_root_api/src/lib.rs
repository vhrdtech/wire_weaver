//! Stands in for an API built on standard library traits (R7) in `wire_weaver_core`'s snapshot tests: every
//! definition is `skipped_dep_api`'s, so codegen leaves them all out against its snapshot.
#![no_std]

use ww_api::prelude::*;

/// Ports and pin banks, nothing of its own.
#[ww_api_root]
pub trait SkippedRoot {
    /// The ports.
    ww_impl!(port[]: skipped_dep_api::Port);
    /// The pin banks.
    ww_impl!(bank[]: skipped_dep_api::Bank);
}

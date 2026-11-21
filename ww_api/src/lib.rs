//! What an API crate needs to describe an API: the macros, the markers and the wire format's prelude, without
//! the runtime.
//!
//! An API crate holds `#[ww_trait]` traits and `#[derive_shrink_wrap]` types, and nothing that runs: no handlers,
//! no events, no transport. It depends on this crate and on the `ww_stdlib` crates and `ww_si` unit groups it
//! uses, and writes `use ww_api::prelude::*;`. Server and client crates take the runtime from `wire_weaver`, whose
//! prelude includes this one, so a server writes `use wire_weaver::prelude::*;` and sees both.
#![no_std]

pub mod api_doc;
mod error;
mod totals;

pub use error::Error;
pub use shrink_wrap;
pub use shrink_wrap::derive_shrink_wrap;
pub use totals::{Accumulate, Totals, TotalsSession};
pub use wire_weaver_derive::{
    compact_version, full_version, ww_api_root, ww_codegen, ww_impl, ww_trait,
};
pub use ww_error;
pub use ww_version;

/// Everything an API crate's `lib.rs` names: `use ww_api::prelude::*;`.
pub mod prelude {
    pub use crate::{Accumulate, Error, Totals, TotalsSession, api_error};
    pub use shrink_wrap;
    pub use shrink_wrap::prelude::*;
    pub use wire_weaver_derive::{
        compact_version, full_version, ww_api_root, ww_codegen, ww_impl, ww_trait,
    };
    pub use ww_error;
    pub use ww_error::CommonError;
    #[cfg(feature = "std")]
    pub use ww_error::CommonErrorOwned;
    pub use ww_version;
    pub use ww_version::FullVersion;
}

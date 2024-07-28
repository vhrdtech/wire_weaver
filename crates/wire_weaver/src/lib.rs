#![cfg_attr(not(feature = "std"), no_std)]

pub use shrink_wrap;
pub use wire_weaver_derive::{wire_weaver, wire_weaver_api, ShrinkWrap};

#[cfg(feature = "std")]
pub mod client_server;

pub mod client_server_no_alloc;

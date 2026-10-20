//! Global id registry: small numbers for commonly used crates and traits, so that a request can name a trait in
//! a few bits instead of its crate name and version (`PathKind::GlobalCompact`). Ids are assigned by hand in
//! `wire_weaver_gid.json`, see the README for the ranges and rules.
#![no_std]

mod gid;
mod registry;

pub use gid::GlobalTypeId;
pub use registry::{Entry, EntryKind, crate_id, entry, trait_id};

include!(concat!(env!("OUT_DIR"), "/gid.rs"));

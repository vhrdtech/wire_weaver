//! API snapshots of WireWeaver crates, found by crate name and version.
//!
//! A device leaves the traits and types known from other crates' snapshots out of its introspection data and names
//! each by crate, version, name and [signature](ww_self::signature); a host puts them back. Codegen leaves one out
//! only when the dependency's own `api_snapshots/` (in its source directory, which the walker locates through
//! `Cargo.lock`) has the same definition. A host finds a snapshot through a chain, in this order:
//!
//! 1. [Set]s the application embedded ([embed()], written at build time by [build]);
//! 2. directories the caller named (codegen and the checker: the walked crates' `api_snapshots/`, and nothing else,
//!    so a build is deterministic given its sources);
//! 3. the cache, `~/.wire_weaver/snapshots/` ([cache]);
//! 4. Cargo's sources on this machine ([cargo]): the workspace a `Cargo.lock` above the current directory names, then
//!    what Cargo fetched under `CARGO_HOME`;
//! 5. a fetch from crates.io or the crate's git repository ([fetch], feature `fetch`; [Resolver::global] has it, the
//!    checker's [Resolver::local] never);
//! 6. nothing: the definition stays skipped, and a value of it is opaque.
//!
//! Every file read passes the checks of [mod@file] (its header's SHA-256, its own crate and version), and a definition
//! from any source is put back only when the signature computed from its bytes matches the reference the device sent
//! (shape, EVO-69): what the device sent is the anchor, no source can make a host decode with another layout. Sizes,
//! counts and timeouts are bounded by [limits].
//!
//! No snapshot of any other crate is compiled into this crate.

pub mod build;
pub mod cache;
pub mod cargo;
pub mod dir;
#[cfg(feature = "fetch")]
pub mod fetch;
pub mod file;
pub mod limits;
mod resolver;
pub mod rustc;
mod set;

pub use dir::{Dir, SNAPSHOTS_DIR, SUPERSEDED_DIR};
pub use file::Snapshot;
pub use resolver::{CargoScan, Origin, Resolver, embed};
pub use set::Set;
pub use ww_self;
pub use ww_version;

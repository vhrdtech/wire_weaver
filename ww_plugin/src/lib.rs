//! What an executable plugin (`ww-NAME` on `PATH`) takes from the `ww` tool: the device arguments ([`DeviceArgs`],
//! resolved with [`Selection::resolve`]: flags, then `WW_*` environment variables, then `ww.toml`), connecting with
//! them ([`plugin::connect`]) and the plugin protocol helpers ([`plugin::complete_if_requested`],
//! [`plugin::selection_env`]).
//!
//! The crate never depends on `ww_hub`, directly or through another crate, so a plugin built on it doesn't link the
//! hub (`tests/layers_lint`, `ww_plugin_is_free_of_the_hub`). `wire_weaver_cli` re-exports all of it at its old
//! paths.

pub mod complete;
pub mod device;
pub mod plugin;
pub mod untrusted;

pub use device::{DeviceArgs, Selection, Setting, Source};
pub use untrusted::{Untrusted, escape};

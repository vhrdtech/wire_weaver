//! The `ww` command line as a library: the generic commands (`list`, `api`, `introspect`, `explain`, `observe`,
//! `stream`, the link tap, `sync`, `hub`, `info`, `elf`, `config`, `settings`), the command [`registry`] every
//! command registers itself with, and what a `ww-<tool>` plugin or a crate adding commands builds on: the device
//! selection ([`device`]), the plugin handshake ([`plugin`]), escaping of untrusted strings ([`untrusted`]) and the
//! shell completion ([`complete`]). A binary that links this crate and the crates adding commands (`ww_std_cli`:
//! the device commands of the standard APIs) runs them all with [`main`].

use anyhow::Result;

mod api_tree;
pub mod cli;
pub mod cmd;
pub mod complete;
pub mod device;
mod dfu_client;
pub mod plugin;
pub mod registry;
pub mod untrusted;
pub mod util;
mod value_fmt;

pub use registry::Program;

/// The `ww` command line: what the `ww` binary runs. `program` is the binary's name and `--version` text.
pub fn main(program: Program) -> Result<()> {
    registry::set_program(program);
    // started by a client as the user's hub (WW_HUB_SERVE set): serves until idle and exits, otherwise does nothing
    ww_hub::main_if_hub();
    // answers shell completion requests (COMPLETE=<shell> set) and exits, otherwise does nothing
    complete::complete(|| complete::with_plugins(cli::root_command));
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?
        .block_on(registry::run())
}

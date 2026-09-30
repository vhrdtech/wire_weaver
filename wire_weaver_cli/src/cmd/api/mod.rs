// mod server_methods;

mod ast;
mod save;

use anyhow::{Result, anyhow};

use clap::{Subcommand, ValueHint};
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum ApiCommand {
    /// Print API tree
    Tree {
        /// Path to crate which defines ww_trait
        #[arg(value_hint = ValueHint::DirPath)]
        path: PathBuf,

        /// Optional trait name if more than one is present
        #[arg(long)]
        name: Option<String>,

        /// Do not print documentation for each resource
        #[arg(short('d'), long)]
        skip_docs: bool,
    },
    ServerMethods {
        /// Path to crate which defines ww_trait
        #[arg(value_hint = ValueHint::DirPath)]
        path: PathBuf,

        /// Optional trait name if more than one is present
        #[arg(long)]
        name: Option<String>,
    },
    /// Save all traits and types defined in a crate into `<path>/api_snapshots/<crate>_<major>_<minor>_<patch>.ron`
    ///
    /// Snapshots are meant to be committed and kept unchanged, so that API bundles (e.g., device introspection data)
    /// can refer to these traits and types by crate name and version only, instead of carrying their definitions.
    /// Traits and types from other crates are only referenced, save a snapshot of each of those crates as well.
    ///
    /// Saving again is a no-op if nothing changed. Any change is an error, doc comments included:
    /// bump the crate version instead (compatible position for doc-only changes).
    Save {
        /// Path to crate which defines ww_trait's and/or data types
        #[arg(value_hint = ValueHint::DirPath)]
        path: PathBuf,

        /// Overwrite an existing snapshot even if traits or types changed without a version bump
        #[arg(long)]
        force: bool,
    },
    /// Print AST
    Ast {
        /// Path to crate which defines ww_trait
        #[arg(value_hint = ValueHint::DirPath)]
        path: PathBuf,

        /// Optional trait name if more than one is present
        #[arg(long)]
        name: Option<String>,
    },
}
pub(crate) fn api(cmd: ApiCommand) -> Result<()> {
    match cmd {
        ApiCommand::Tree {
            path,
            name,
            skip_docs,
        } => {
            let bundle = wire_weaver_core::load(&path, name, false)?;
            print!("{}", crate::api_tree::render(&bundle, skip_docs));
            Ok(())
        }
        // ApiCommand::ServerMethods { path, name } => server_methods::server_methods(path, name),
        ApiCommand::ServerMethods { .. } => Err(anyhow!("Not implemented yet")),
        ApiCommand::Save { path, force } => save::save(path, force),
        ApiCommand::Ast { path, name } => ast::print_ast(path, name),
    }
}

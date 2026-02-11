mod server_methods;
mod tree_printer;

use anyhow::Result;

use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum ApiCommand {
    /// Print API tree
    Tree {
        /// Path to file which defines ww_trait
        path: PathBuf,

        /// Optional trait name if more than one is present
        #[arg(long)]
        name: Option<String>,

        /// Skip reserved resources
        #[arg(short('r'), long)]
        skip_reserved: bool,

        /// Do not print documentation for each resource
        #[arg(short('d'), long)]
        skip_docs: bool,
    },
    ServerMethods {
        /// Path to file which defines ww_trait
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
            skip_reserved,
            skip_docs,
        } => tree_printer::tree_printer(path, name, skip_reserved, skip_docs)?,
        ApiCommand::ServerMethods { path, name } => server_methods::server_methods(path, name)?,
    }
    Ok(())
}

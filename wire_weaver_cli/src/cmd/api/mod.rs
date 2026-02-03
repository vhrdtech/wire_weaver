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
        #[arg(short, long)]
        skip_reserved: bool,
    },
}
pub(crate) fn api(cmd: ApiCommand) -> Result<()> {
    match cmd {
        ApiCommand::Tree {
            path,
            name,
            skip_reserved,
        } => tree_printer::tree_printer(path, name, skip_reserved)?,
    }
    Ok(())
}

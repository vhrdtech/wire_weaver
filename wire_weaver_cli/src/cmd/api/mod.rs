// mod server_methods;

mod ast;

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
        ApiCommand::Ast { path, name } => ast::print_ast(path, name),
    }
}

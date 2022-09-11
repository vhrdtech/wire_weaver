use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
pub struct Cli {
    #[clap(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Generate some code
    #[clap(aliases = &["g", "gen"])]
    Generate { vhl_source: PathBuf },

    /// Developer tools
    Dev {
        /// Print lexer output (Pest pairs)
        #[clap(short, long)]
        lexer: bool,

        /// Print parser output (core AST)
        #[clap(short, long)]
        parser: bool,

        /// Optional definition to filter out, otherwise whole file is shown
        #[clap(short, long)]
        definition: Option<String>,

        /// Source file path, local path starting with /, file:, git: or reg:
        #[clap(value_parser)]
        vhl_source: String,
    },

    /// REPL
    #[clap(alias = "rxpi")]
    ReplXpi(ReplXpi),
}

#[derive(Args)]
pub struct ReplXpi {
    /// Source file path, local path starting with /, file:, git: or reg:
    #[clap(value_parser)]
    pub vhl_source: String,
}

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
    #[clap(aliases = & ["g", "gen"])]
    Generate(GenerateArgs),

    /// Developer tools
    Dev(DevArgs),

    /// REPL
    #[clap(alias = "repl")]
    Repl(ReplArgs),
}

#[derive(Args)]
pub struct GenerateArgs {
    /// Source file path, local path starting with /, file:, git: or reg:
    #[clap(value_parser)]
    pub vhl_source: PathBuf
}

#[derive(Args)]
pub struct ReplArgs {
    /// Source file path, local path starting with /, file:, git: or reg:
    #[clap(value_parser)]
    pub vhl_source: String,
}

#[derive(Args)]
pub struct DevArgs {
    /// Print lexer output (Pest pairs)
    #[clap(short, long)]
    pub lexer: bool,

    /// Print parser output (core AST)
    #[clap(short, long)]
    pub parser: bool,

    /// Do full processing of the AST
    #[clap(long)]
    pub process: bool,

    /// Optional definition to filter out, otherwise whole file is shown
    #[clap(short, long)]
    pub definition: Option<String>,

    /// Source file path, local path starting with /, file:, git: or reg:
    #[clap(value_parser)]
    pub vhl_source: String,
}
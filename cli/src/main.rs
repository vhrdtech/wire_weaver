mod commands;
mod handlers;
pub mod util;

use crate::commands::Commands;
use anyhow::{anyhow, Result};
use clap::Parser;

fn main() -> Result<()> {
    let cli = commands::Cli::parse();

    match cli.command {
        Some(Commands::Generate(generate_args)) => {
            handlers::generate::generate_subcmd(generate_args)
        }
        Some(Commands::Dev(dev_args)) => handlers::dev::dev_subcmd(dev_args),
        Some(Commands::Repl(repl_xpi)) => handlers::repl::repl_xpi_cmd(repl_xpi),
        None => Err(anyhow!("Subcommand expected")),
    }
}

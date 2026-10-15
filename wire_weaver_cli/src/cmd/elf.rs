//! `ww elf`: the ELF directory, where firmware ELFs are found by build id (CLT-25). The lookup and the unpacking
//! of release archives are `cnt_core::elf`, shared with `cnt`.

use anyhow::{Context, Result};
use clap::{Subcommand, ValueHint};
use cnt_core::elf;
use console::style;
use std::path::PathBuf;

#[derive(Subcommand)]
pub(crate) enum ElfCommand {
    /// Add ELFs to the ELF directory (~/.cache/ww/elf), stored by build id: single ELFs, or a firmware release's
    /// ELF archive (`<firmware>-<version>-elf.tar.zst`, or .tar)
    Add {
        /// ELF files or release archives
        #[arg(required = true, value_hint = ValueHint::FilePath)]
        paths: Vec<PathBuf>,
        /// Directory to add them to instead (e.g. one listed in WW_ELF_PATH)
        #[arg(long, value_hint = ValueHint::DirPath)]
        dir: Option<PathBuf>,
    },
    /// Print the build id of an ELF, and whether it is in the ELF directory
    Id {
        #[arg(value_hint = ValueHint::FilePath)]
        path: PathBuf,
    },
}

pub(crate) fn elf(cmd: ElfCommand) -> Result<()> {
    match cmd {
        ElfCommand::Add { paths, dir } => {
            let dir = match dir {
                Some(dir) => dir,
                None => {
                    elf::cache_dir().context("No cache directory on this system, pass --dir")?
                }
            };
            for path in paths {
                let (added, skipped) = elf::add(&path, &dir)?;
                for a in added {
                    println!(
                        "{} {} {}",
                        elf::hex(&a.build_id),
                        style("from").dim(),
                        a.from
                    );
                }
                for s in skipped {
                    println!(
                        "⚠️ {}",
                        style(format!("{s} has no build id, skipped")).yellow()
                    );
                }
            }
            println!("{} {}", style("in").dim(), style(dir.display()).cyan());
        }
        ElfCommand::Id { path } => {
            let bytes = std::fs::read(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            match elf::build_id(&bytes)? {
                Some(id) => match elf::find_in_dirs(&id, &elf::elf_dirs()) {
                    Some(p) => println!(
                        "{} {} {}",
                        elf::hex(&id),
                        style("in").dim(),
                        style(p.display()).cyan()
                    ),
                    None => println!(
                        "{} {}",
                        elf::hex(&id),
                        style("(not in the ELF directory)").dim()
                    ),
                },
                None => println!(
                    "⚠️ {}",
                    style(format!("{} has no build id", path.display())).yellow()
                ),
            }
        }
    }
    Ok(())
}

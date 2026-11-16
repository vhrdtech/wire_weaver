//! `ww-book assemble`: copies the pages and feature trackers `book_sources.toml` lists into `docs/` (run from the
//! repository root, as `just book-assemble` does).

use anyhow::{Result, bail};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args != ["assemble"] {
        bail!(
            "usage: ww-book assemble (copies the pages of {} into docs/)",
            ww_book::SOURCES_FILE
        );
    }
    let root = std::env::current_dir()?;
    let sources = ww_book::locate(&root)?;
    let written = ww_book::assemble(&root, &sources)?;
    for source in &sources {
        println!(
            "{} pages from {} into docs/{}",
            source.source.package,
            source.pages_dir().display(),
            source.source.into.display()
        );
    }
    println!("{} files written", written.len());
    Ok(())
}

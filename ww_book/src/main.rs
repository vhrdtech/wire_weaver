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
    let sources = ww_book::locate(&root, ww_book::Update::Fetch)?;
    let written = ww_book::assemble(&root, &sources)?;
    for source in &sources {
        let at = match &source.commit {
            Some((commit, None)) => format!(" at {commit}"),
            Some((commit, Some(why))) => {
                format!(" at {commit}, the checkout as it was: fetching failed ({why})")
            }
            None => String::new(),
        };
        println!(
            "{} pages from {} into docs/{}{at}",
            source.source.name(),
            source.pages_dir().display(),
            source.source.into.display()
        );
    }
    let sources_file = ww_book::write_sources_file(&root, &sources)?;
    println!(
        "{} files written, code sources in {}",
        written.len(),
        sources_file.display()
    );
    Ok(())
}

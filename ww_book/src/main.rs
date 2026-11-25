//! `ww-book assemble`: copies the pages and feature trackers `book_sources.toml` lists into `docs/`. `ww-book
//! check`: the book's frame, as `ww_book::check` says (run both from the repository root, as `just book-assemble`
//! and `just book-check` do).

use anyhow::{Result, bail};
use std::process::ExitCode;

fn main() -> Result<ExitCode> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = std::env::current_dir()?;
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["assemble"] => assemble(&root)?,
        ["check"] => {
            let sources = ww_book::locate(&root, ww_book::Update::Keep)?;
            let problems = ww_book::check::check(&root, &sources)?;
            if !problems.0.is_empty() {
                eprint!("{problems}");
                eprintln!("{} problems in the book's frame", problems.0.len());
                return Ok(ExitCode::FAILURE);
            }
            println!("the book's frame is in order");
        }
        _ => bail!(
            "usage: ww-book assemble (copies the pages of {} into docs/) | ww-book check (the nav's level order, \
             levels, bare `rust` fences, generated files; {})",
            ww_book::SOURCES_FILE,
            ww_book::check::CHECKS_FILE
        ),
    }
    Ok(ExitCode::SUCCESS)
}

fn assemble(root: &std::path::Path) -> Result<()> {
    let sources = ww_book::locate(root, ww_book::Update::Fetch)?;
    let written = ww_book::assemble(root, &sources)?;
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
    let sources_file = ww_book::write_sources_file(root, &sources)?;
    println!(
        "{} files written, code sources in {}",
        written.len(),
        sources_file.display()
    );
    Ok(())
}

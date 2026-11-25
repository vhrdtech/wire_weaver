//! A small, standalone copy of `tests/book_outputs`' stale check: this crate is excluded from the main workspace
//! (its own `Cargo.lock`, so ciborium/prost/wincode/planus/rasn never reach every other build's lockfile), so it
//! cannot depend on the `book_outputs` crate. It keeps the same contract (`docs/generated/<name>`,
//! `WW_BOOK_OUTPUTS=write`) so `just book-outputs` and `just test` drive it, outside of nextest's hang detector.

use std::fs;
use std::path::{Path, PathBuf};

const WRITE_ENV: &str = "WW_BOOK_OUTPUTS";
const GENERATED_DIR: &str = "docs/generated";

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|dir| dir.join("zensical.toml").is_file())
        .expect("compare_wire_formats sits inside the repository of the book")
        .to_path_buf()
}

/// Writes `text` to `docs/generated/<name>` when `WW_BOOK_OUTPUTS=write`, else fails if the file differs.
///
/// # Panics
/// When the file is stale or cannot be written.
pub fn check(name: &str, text: &str) {
    let write = std::env::var(WRITE_ENV).is_ok_and(|value| value == "write");
    let file = repository_root().join(GENERATED_DIR).join(name);
    if write {
        fs::create_dir_all(file.parent().expect("the file has a directory"))
            .expect("create the directory");
        fs::write(&file, text).expect("write the generated file");
        return;
    }
    let on_disk = fs::read_to_string(&file).unwrap_or_default();
    assert_eq!(
        on_disk,
        text,
        "{} is stale: run `just book-outputs`",
        file.display()
    );
}

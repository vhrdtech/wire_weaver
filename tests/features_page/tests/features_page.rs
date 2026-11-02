//! `docs/features_today.md` matches `FEATURES.md`, `exact_units/FEATURES.md` and `entries.md` (DOC-7, DOC-9).
//! `FEATURES_PAGE=update` (`just features-page`) writes it.

use std::path::{Path, PathBuf};

/// `exact_units` is a plain git dependency (`Cargo.toml`), no local path override: its source, and so its
/// `FEATURES.md`, lives wherever Cargo put the checkout it pinned in `Cargo.lock`
/// (`CARGO_HOME/git/checkouts/exact_units-*/<rev prefix>*/`), the same lookup the crate walker uses for the wire
/// format (`wire_weaver_core::transform::cargo_source`), not a path relative to this repository.
fn exact_units_dir(root: &Path) -> PathBuf {
    let lock = std::fs::read_to_string(root.join("Cargo.lock")).unwrap();
    let rev = exact_units_git_rev(&lock);
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").expect("CARGO_HOME or HOME must be set"))
                .join(".cargo")
        });
    let checkouts = cargo_home.join("git").join("checkouts");
    for repo_dir in std::fs::read_dir(&checkouts)
        .unwrap_or_else(|e| panic!("{}: {e}", checkouts.display()))
        .flatten()
    {
        if !repo_dir
            .file_name()
            .to_string_lossy()
            .starts_with("exact_units-")
        {
            continue;
        }
        if let Some(dir) = std::fs::read_dir(repo_dir.path())
            .into_iter()
            .flatten()
            .flatten()
            .find_map(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                (name.len() >= 7 && rev.starts_with(name.as_ref())).then(|| e.path())
            })
        {
            return dir;
        }
    }
    panic!(
        "no checkout of exact_units at {rev} in {}/exact_units-*/: run `cargo fetch` first",
        checkouts.display()
    );
}

/// The git revision `Cargo.lock` pinned `exact_units` to, from its `[[package]]` entry's `source =
/// "git+URL?rev=..#SHA"`.
fn exact_units_git_rev(lock: &str) -> &str {
    let mut in_exact_units = false;
    for line in lock.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            in_exact_units = false;
            continue;
        }
        if line == "name = \"exact_units\"" {
            in_exact_units = true;
            continue;
        }
        if in_exact_units
            && let Some(source) = line
                .strip_prefix("source = \"")
                .and_then(|s| s.strip_suffix('"'))
            && let Some(git) = source.strip_prefix("git+")
        {
            let (_, rev) = git
                .split_once('#')
                .unwrap_or_else(|| panic!("git source without a revision: {source}"));
            return rev;
        }
    }
    panic!("exact_units is not a git dependency in Cargo.lock: run `cargo fetch` first");
}

#[test]
fn features_page_is_current() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.join("../..");
    let features = std::fs::read_to_string(root.join("FEATURES.md")).unwrap();
    let exact_units_features =
        std::fs::read_to_string(exact_units_dir(&root).join("FEATURES.md")).unwrap();
    let entries = std::fs::read_to_string(manifest.join("entries.md")).unwrap();
    let page = match features_page::render(&[&features, &exact_units_features], &entries) {
        Ok(page) => page,
        Err(problems) => panic!("the features page can't be made:\n{problems}"),
    };
    let path = root.join("docs/features_today.md");
    if std::env::var("FEATURES_PAGE").as_deref() == Ok("update") {
        std::fs::write(&path, page).unwrap();
        return;
    }
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        current == page,
        "docs/features_today.md doesn't match FEATURES.md and tests/features_page/entries.md: run `just features-page`"
    );
}

#[test]
fn entries_link_to_pages_that_exist() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let entries = std::fs::read_to_string(manifest.join("entries.md")).unwrap();
    let docs = manifest.join("../../docs");
    let missing: Vec<String> = features_page::links(&entries)
        .into_iter()
        .filter(|page| !docs.join(page).is_file())
        .collect();
    assert!(
        missing.is_empty(),
        "entries.md links to pages that aren't in docs/: {missing:?}"
    );
}

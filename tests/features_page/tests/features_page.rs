//! `docs/features_today.md` matches `FEATURES.md`, the trackers of the repositories in `book_sources.toml`
//! (shrink_wrap's), and `entries.md` (DOC-7, DOC-9). `FEATURES_PAGE=update` (`just features-page`) writes it.

use std::path::Path;

/// This repository's `FEATURES.md`, then the tracker of each repository whose pages the book assembles: (page in
/// the book, whose, text).
fn trackers(root: &Path) -> Vec<(String, Option<String>, String)> {
    let mut trackers = vec![(
        "features.md".to_owned(),
        None,
        std::fs::read_to_string(root.join("FEATURES.md")).unwrap(),
    )];
    for source in ww_book::locate(root).unwrap() {
        if let Some(tracker) = source.tracker().unwrap() {
            let page = source.source.into.join("features.md");
            trackers.push((
                page.to_string_lossy().into_owned(),
                Some(source.source.name().to_owned()),
                tracker,
            ));
        }
    }
    trackers
}

#[test]
fn features_page_is_current() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.join("../..");
    let trackers = trackers(&root);
    let sources: Vec<features_page::TrackerSource<'_>> = trackers
        .iter()
        .map(|(page, name, text)| features_page::TrackerSource {
            page,
            name: name.as_deref(),
            text,
        })
        .collect();
    let entries = std::fs::read_to_string(manifest.join("entries.md")).unwrap();
    let page = match features_page::render(&sources, &entries) {
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
    let root = manifest.join("../..");
    let sources = ww_book::locate(&root).unwrap();
    let missing: Vec<String> = features_page::links(&entries)
        .into_iter()
        .filter(|page| ww_book::find_page(&root, &sources, Path::new(page)).is_none())
        .collect();
    assert!(
        missing.is_empty(),
        "entries.md links to pages that are neither in docs/ nor in a repository of book_sources.toml: {missing:?}"
    );
}

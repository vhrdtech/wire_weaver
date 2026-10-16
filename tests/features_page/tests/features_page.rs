//! `docs/features_today.md` matches `FEATURES.md` and `entries.md` (DOC-7, DOC-9). `FEATURES_PAGE=update`
//! (`just features-page`) writes it.

use std::path::Path;

#[test]
fn features_page_is_current() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.join("../..");
    let features = std::fs::read_to_string(root.join("FEATURES.md")).unwrap();
    let entries = std::fs::read_to_string(manifest.join("entries.md")).unwrap();
    let page = match features_page::render(&features, &entries) {
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

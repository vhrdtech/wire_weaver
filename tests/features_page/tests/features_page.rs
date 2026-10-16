//! `docs/features_today.md` matches `FEATURES.md` (DOC-7). `FEATURES_PAGE=update` (`just features-page`) writes it.

use std::path::Path;

#[test]
fn features_page_is_current() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let features = std::fs::read_to_string(root.join("FEATURES.md")).unwrap();
    let page = features_page::render(&features);
    let path = root.join("docs/features_today.md");
    if std::env::var("FEATURES_PAGE").as_deref() == Ok("update") {
        std::fs::write(&path, page).unwrap();
        return;
    }
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        current == page,
        "docs/features_today.md doesn't match FEATURES.md: run `just features-page`"
    );
}

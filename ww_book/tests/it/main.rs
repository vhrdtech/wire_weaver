//! The book of this repository: its frame passes `ww-book check`.

use std::path::Path;

#[test]
fn the_book_frame_is_in_order() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("ww_book sits in the repository root");
    let sources = ww_book::locate(root, ww_book::Update::Keep).unwrap();
    let problems = ww_book::check::check(root, &sources).unwrap();
    assert!(
        problems.0.is_empty(),
        "the book's frame (book_checks.toml, zensical.toml's nav):\n{problems}"
    );
}

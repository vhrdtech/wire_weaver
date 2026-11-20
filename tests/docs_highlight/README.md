# docs_highlight

Checks the docs site's vendored WireWeaver syntax highlighter (DOC-11) against the `ww_lsp` crate's `Cargo.lock` pin.

`UPDATE_VENDORED=1 cargo nextest run -p docs_highlight` (after `cargo update -p ww_highlight`) refreshes the vendored copies.

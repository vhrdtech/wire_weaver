//! The docs site's WireWeaver syntax highlighting (DOC-11) is vendored from ww_lsp's `ww_highlight` crate:
//! `docs/javascripts/ww_highlight.js`, `docs/javascripts/ww_tokens.json` and `docs/stylesheets/ww_highlight.css`.
//! The test here fails when a copy differs from what the ww_lsp of `Cargo.lock` gives; `UPDATE_VENDORED=1 cargo
//! nextest run -p docs_highlight` writes the copies anew (after `cargo update -p ww_highlight`).

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn vendored_copies_match_ww_lsp() {
        let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
        let copies = [
            ("javascripts/ww_highlight.js", ww_highlight::JS.to_owned()),
            ("javascripts/ww_tokens.json", ww_highlight::tokens_json()),
            ("stylesheets/ww_highlight.css", ww_highlight::CSS.to_owned()),
        ];
        for (path, want) in copies {
            let path = docs.join(path);
            if std::env::var_os("UPDATE_VENDORED").is_some() {
                std::fs::write(&path, &want).unwrap();
                continue;
            }
            let got = std::fs::read_to_string(&path).unwrap_or_default();
            assert!(
                got == want,
                "{} differs from ww_lsp's ww_highlight; UPDATE_VENDORED=1 writes it anew",
                path.display()
            );
        }
    }
}

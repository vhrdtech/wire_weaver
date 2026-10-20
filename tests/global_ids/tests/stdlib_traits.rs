//! Every public `#[ww_trait]` / `#[ww_api_root]` in `ww_stdlib` has an id in `ww_global/wire_weaver_gid.json` (or is
//! listed in its `no_id`), and every trait entry of a `ww_stdlib` crate names a trait that exists (or is retired).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use ww_global::{ENTRIES, EntryKind, NO_ID};

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn is_ww_trait(attr: &syn::Attribute) -> bool {
    attr.path()
        .segments
        .last()
        .is_some_and(|s| s.ident == "ww_trait" || s.ident == "ww_api_root")
}

fn traits_in(items: &[syn::Item], crate_name: &str, out: &mut BTreeSet<String>) {
    for item in items {
        match item {
            syn::Item::Trait(t)
                if matches!(t.vis, syn::Visibility::Public(_))
                    && t.attrs.iter().any(is_ww_trait) =>
            {
                out.insert(format!("{crate_name}::{}", t.ident));
            }
            syn::Item::Mod(m) => {
                if let Some((_, items)) = &m.content {
                    traits_in(items, crate_name, out);
                }
            }
            _ => {}
        }
    }
}

/// `crate::Trait` of every public API trait in `ww_stdlib`, and the crate names.
fn stdlib_traits() -> (BTreeSet<String>, BTreeSet<String>) {
    let stdlib = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ww_stdlib");
    let (mut traits, mut crates) = (BTreeSet::new(), BTreeSet::new());
    for entry in std::fs::read_dir(&stdlib).unwrap() {
        let dir = entry.unwrap().path();
        let Ok(manifest) = std::fs::read_to_string(dir.join("Cargo.toml")) else {
            continue;
        };
        let manifest: toml::Table = manifest.parse().unwrap();
        let crate_name = manifest["package"]["name"].as_str().unwrap().to_string();
        let mut files = vec![];
        rs_files(&dir.join("src"), &mut files);
        for file in files {
            let src = std::fs::read_to_string(&file).unwrap();
            let file = syn::parse_file(&src).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
            traits_in(&file.items, &crate_name, &mut traits);
        }
        crates.insert(crate_name);
    }
    (traits, crates)
}

#[test]
fn every_stdlib_trait_has_an_id() {
    let (traits, crates) = stdlib_traits();
    assert!(
        traits.len() > 40,
        "found only {} traits, is the walk broken?",
        traits.len()
    );
    let mut errors = vec![];
    for name in &traits {
        let (crate_name, trait_name) = name.split_once("::").unwrap();
        if ww_global::trait_id(crate_name, trait_name).is_none() && !NO_ID.contains(&name.as_str())
        {
            errors.push(format!(
                "`{name}` has no id: add it to ww_global/wire_weaver_gid.json (see the ranges in its README) or to `no_id`"
            ));
        }
    }
    for e in ENTRIES
        .iter()
        .filter(|e| e.kind == EntryKind::Trait && e.retired.is_none())
    {
        let (crate_name, _) = e.split();
        if crates.contains(crate_name) && !traits.contains(e.name) {
            errors.push(format!(
                "id {} names `{}`, which is not a public ww_trait in ww_stdlib: rename (old name into `aliases`) or mark it `retired`",
                e.id, e.name
            ));
        }
    }
    for name in NO_ID {
        assert!(
            traits.contains(*name),
            "no_id lists `{name}`, which is not a public ww_trait in ww_stdlib"
        );
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

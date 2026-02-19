use anyhow::{anyhow, Result};
use proc_macro2::{Ident, Span};
use std::collections::HashMap;
use std::path::PathBuf;
use wire_weaver_core::ast::api::ApiLevel;
use wire_weaver_core::ast::trait_macro_args::ImplTraitLocation;
use wire_weaver_core::transform::load::load_api_level_recursive;

pub(crate) fn load_level(path: PathBuf, name: Option<String>) -> Result<ApiLevel> {
    // do some gymnastics to point base_dir at crate root (where Cargo.toml is)
    let mut base_dir = path.clone();
    base_dir.pop(); // pop ww.rs
    base_dir.pop(); // pop src

    let parent = path
        .parent()
        .unwrap()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap(); // likely src folder
    let file_name = path.file_name().unwrap().to_str().unwrap(); // likely ww.rs or src.rs

    let mut cache = HashMap::new();
    let level = load_api_level_recursive(
        &ImplTraitLocation::AnotherFile {
            path: format!("{parent}/{file_name}"),
            part_of_crate: Ident::new("crate", Span::call_site()),
        },
        name.map(|n| Ident::new(n.as_str(), Span::call_site())),
        None,
        base_dir.as_path(),
        &mut cache,
    )
    .map_err(|e| anyhow!(e))?;
    Ok(level)
}

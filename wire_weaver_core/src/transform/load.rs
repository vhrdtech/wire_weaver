use crate::ast::api::{ApiItemKind, ApiLevel, ApiLevelSourceLocation};
use crate::ast::trait_macro_args::ImplTraitLocation;
use crate::transform::transform_api_level::transform_api_level;
use relative_path::RelativePath;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn load_api_level_recursive(
    location: &ImplTraitLocation,
    trait_name: Option<proc_macro2::Ident>,
    source_location: Option<ApiLevelSourceLocation>,
    base_dir: &Path,
    cache: &mut HashMap<PathBuf, syn::File>,
) -> Result<ApiLevel, String> {
    let (path, source_location) = match location {
        ImplTraitLocation::SameFile => {
            if let Some(source_location) = &source_location {
                match source_location {
                    ApiLevelSourceLocation::File {
                        path,
                        part_of_crate: _,
                    } => (path.clone(), source_location.clone()),
                    ApiLevelSourceLocation::Crate { .. } => todo!(),
                }
            } else {
                // Get the `Span` where the macro was called
                // let span = proc_macro::Span::call_site();
                // let source_file = span.source_file();
                // let path: PathBuf = source_file.path();
                // TODO: implement same file item reference when proc_macro_span is stabilized
                // see: https://github.com/rust-lang/rust/issues/54725
                return Err(
                    "please use a file path to this source instead of referring to the item directly (proc_macro_span is not available yet)".into()
                );
            }
        }
        ImplTraitLocation::AnotherFile {
            path,
            part_of_crate,
        } => {
            // if trait_source_str.starts_with('.') || trait_source_str.starts_with("..") {
            let path = RelativePath::new(path.as_str()).to_path(base_dir);
            let source_location = ApiLevelSourceLocation::File {
                path: path.clone(),
                part_of_crate: part_of_crate.clone(),
            };
            (path, source_location)
        }
        ImplTraitLocation::CratesIo { .. } => {
            todo!()
        }
    };
    let ast = if let Some(ast) = cache.get(&path) {
        ast
    } else {
        let contents = match std::fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(e) => {
                return Err(format!("Error reading source file at {path:?}: {e:?}"));
            }
        };
        let ast: syn::File = syn::parse_str(&contents).map_err(|e| format!("{path:?}: {e}"))?;
        cache.insert(path.clone(), ast);
        cache.get(&path).expect("")
    };

    let mut api_level = None;
    for item in &ast.items {
        if let syn::Item::Trait(item_trait) = item {
            if !item_trait
                .attrs
                .iter()
                .any(|attr| attr.path().is_ident("ww_trait"))
            {
                continue;
            }
            if let Some(trait_name) = &trait_name
                && &item_trait.ident != trait_name
            {
                continue;
            }
            if api_level.is_some() {
                return Err("Multiple traits with the same name".into());
            }
            let level = transform_api_level(item_trait, source_location.clone())?;
            api_level = Some(level);
        }
    }

    if let Some(mut level) = api_level {
        for item in &mut level.items {
            if let ApiItemKind::ImplTrait { args, level } = &mut item.kind {
                let base_dir = if let ApiLevelSourceLocation::File { path, .. } = &source_location {
                    let mut path_to_file_in_src = path.clone();
                    path_to_file_in_src.pop(); // pop ww.rs
                    path_to_file_in_src.pop(); // pop src
                    path_to_file_in_src
                } else {
                    base_dir.to_path_buf()
                };
                let inner_level = load_api_level_recursive(
                    &args.location,
                    Some(args.trait_name.clone()),
                    Some(source_location.clone()),
                    &base_dir,
                    cache,
                )?;
                *level = Some(Box::new(inner_level));
            }
        }
        Ok(level)
    } else if let Some(trait_name) = trait_name {
        Err(format!("ww_trait `{}` not found", trait_name.to_string()))
    } else {
        Err("ww_trait not found".into())
    }
}

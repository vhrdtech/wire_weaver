use crate::ww_impl_args::ApiArgs;
use proc_macro2::{Span, TokenStream};
use quote::TokenStreamExt;
use relative_path::RelativePath;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use wire_weaver_core::ast::api::{ApiItemKind, ApiLevel, ApiLevelSourceLocation};
use wire_weaver_core::ast::trait_macro_args::ImplTraitLocation;
use wire_weaver_core::codegen::api_client::{ClientModel, ClientPathMode};
use wire_weaver_core::method_model::{MethodModel, MethodModelKind};
use wire_weaver_core::property_model::{PropertyModel, PropertyModelKind};
use wire_weaver_core::transform::transform_api_level::transform_api_level;

pub fn ww_api(args: ApiArgs) -> TokenStream {
    api_inner(args, true)
        .unwrap_or_else(|e| syn::Error::new(Span::call_site(), e).to_compile_error())
}

pub fn ww_impl(args: ApiArgs) -> TokenStream {
    api_inner(args, false)
        .unwrap_or_else(|e| syn::Error::new(Span::call_site(), e).to_compile_error())
}

fn api_inner(args: ApiArgs, is_root: bool) -> Result<TokenStream, String> {
    let mut cache = HashMap::new();
    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("env variable CARGO_MANIFEST_DIR should be set"),
    );
    let mut level = load_api_level_recursive(
        &args.location,
        args.trait_name.clone(),
        None,
        &manifest_dir,
        &mut cache,
    )?;

    if !args.ext.no_alloc {
        level.make_owned();
    }

    let property_model = if args.ext.property_model.is_empty() {
        PropertyModel {
            default: Some(PropertyModelKind::GetSet),
            items: vec![],
        }
    } else {
        PropertyModel::parse(&args.ext.property_model)
            .map_err(|e| format!("failed to parse property model: {e}"))?
    };
    let method_model = if args.ext.method_model.is_empty() {
        MethodModel {
            default: Some(MethodModelKind::Immediate),
            items: vec![],
        }
    } else {
        MethodModel::parse(&args.ext.method_model)
            .map_err(|e| format!("failed to parse method model: {e}"))?
    };

    let mut codegen_ts = TokenStream::new();
    if args.ext.server {
        let ts = wire_weaver_core::codegen::api_server::impl_server_dispatcher(
            &level,
            args.ext.no_alloc,
            args.ext.use_async,
            &method_model,
            &property_model,
            &args.context_ident,
            &syn::Ident::new("process_request_bytes", Span::call_site()),
        );
        codegen_ts.append_all(ts);
    }

    if !args.ext.client.is_empty() {
        let model = match args.ext.client.as_str() {
            "raw" => ClientModel::Raw,
            "async_worker" => ClientModel::AsyncWorker,
            _ => {
                return Err(format!(
                    "client supports raw or async_worked modes, got: '{}'",
                    args.ext.client
                ));
            }
        };
        let path_mode = if is_root {
            ClientPathMode::Absolute
        } else {
            ClientPathMode::GlobalTrait
        };
        let ts = wire_weaver_core::codegen::api_client::client(
            &level,
            model,
            path_mode,
            &args.context_ident,
        );
        codegen_ts.append_all(ts);
    }

    if !args.ext.debug_to_file.is_empty() {
        let path = manifest_dir.join(&args.ext.debug_to_file);
        match File::create(&path) {
            Ok(mut f) => {
                let level_debug = format!("{:#?}", &level);
                for line in level_debug.split('\n') {
                    f.write_fmt(format_args!("// {line}\n"))
                        .map_err(|e| e.to_string())?;
                }
                let ts_formatted = crate::util::format_rust(format!("{codegen_ts}").as_str());
                f.write_all(ts_formatted.as_bytes())
                    .map_err(|e| e.to_string())?;
            }
            Err(e) => {
                return Err(format!("Debug file create failed: {path:?} {:?}", e));
            }
        }
    }

    Ok(codegen_ts)
}

fn load_api_level_recursive(
    location: &ImplTraitLocation,
    trait_name: proc_macro2::Ident,
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
            if item_trait.ident != trait_name {
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
                    args.trait_name.clone(),
                    Some(source_location.clone()),
                    &base_dir,
                    cache,
                )?;
                *level = Some(Box::new(inner_level));
            }
        }
        Ok(level)
    } else {
        Err("ww_trait not found".into())
    }
}

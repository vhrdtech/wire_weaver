use crate::api;
use darling::Error;
use proc_macro2::TokenStream;
use quote::TokenStreamExt;
use relative_path::RelativePath;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use wire_weaver_core::ast::Item;
use wire_weaver_core::method_model::{MethodModel, MethodModelKind};
use wire_weaver_core::property_model::{PropertyModel, PropertyModelKind};
use wire_weaver_core::transform::Transform;

pub fn ww_impl(args: crate::ww_impl_args::ImplArgs) -> TokenStream {
    let mut transform = Transform::new();
    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("env variable CARGO_MANIFEST_DIR should be set"),
    );
    let trait_source_str = args.trait_source.value();
    if trait_source_str.starts_with('.') || trait_source_str.starts_with("..") {
        let ww_path = RelativePath::new(trait_source_str.as_str())
            .to_path(&manifest_dir)
            .to_str()
            .expect("path to user ww file is not valid Unicode")
            .to_owned();
        let contents = match std::fs::read_to_string(ww_path.as_str()) {
            Ok(contents) => contents,
            Err(e) => {
                return Error::custom(format!("Error reading ww source file: {e:?}"))
                    .with_span(&args.trait_source.span())
                    .write_errors();
            }
        };
        let ast = match syn::parse_file(contents.as_str()) {
            Ok(ast) => ast,
            Err(e) => {
                return Error::custom(format!("Error parsing ww source file: {e:?}"))
                    .with_span(&args.trait_source.span())
                    .write_errors();
            }
        };
        let mut matched = 0;
        for item in ast.items {
            if let syn::Item::Trait(item_trait) = item {
                if item_trait.ident != args.trait_name {
                    continue;
                }
                transform.push_trait(item_trait);
                matched += 1;
            }
        }
        if matched == 0 {
            return syn::Error::new(
                args.trait_source.span(),
                format!("Trait {} not found", args.trait_name),
            )
            .to_compile_error();
        }
        if matched > 1 {
            return Error::custom(format!(
                "Multiple traits with the name {} found",
                args.trait_name
            ))
            .with_span(&args.trait_source.span())
            .write_errors();
        }
    } else {
        let trait_name_version = trait_source_str.split(':').collect::<Vec<&str>>();
        if trait_name_version.len() != 2 {
            return Error::custom("Expected crates.io \"crate_name:x.y.z\" or \"./path/to/src.ww\" or \"../path/to/src.ww\"")
                .with_span(&args.trait_source.span())
                .write_errors();
        }
        let _crate_name = trait_name_version[0];
        let _version = trait_name_version[1];
        return Error::custom(format!(
            "crates.io loading is not supported yet {_crate_name} {_version}"
        ))
        .write_errors();
    }

    let add_derives = args
        .ext
        .derive
        .split(&[' ', ','])
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();

    let cx = transform
        .transform(&add_derives, false)
        .expect("ww transform failed");
    for (source, messages) in transform.messages() {
        for message in messages.messages() {
            println!("{:?} {:?}", source, message);
        }
    }

    let property_model = if args.ext.property_model.is_empty() {
        PropertyModel {
            default: Some(PropertyModelKind::GetSet),
            items: vec![],
        }
    } else {
        PropertyModel::parse(&args.ext.property_model).unwrap()
    };
    let method_model = if args.ext.method_model.is_empty() {
        MethodModel {
            default: Some(MethodModelKind::Immediate),
            items: vec![],
        }
    } else {
        MethodModel::parse(&args.ext.method_model).unwrap()
    };

    let mut codegen_ts = TokenStream::new();
    for module in &cx.modules {
        for item in &module.items {
            match item {
                Item::Struct(item_struct) => {
                    let ts = wire_weaver_core::codegen::item_struct::struct_def(
                        item_struct,
                        args.ext.no_alloc,
                    );
                    codegen_ts.append_all(ts);

                    let ts = wire_weaver_core::codegen::item_struct::struct_serdes(
                        item_struct,
                        args.ext.no_alloc,
                    );
                    codegen_ts.append_all(ts);
                }
                Item::Enum(item_enum) => {
                    let ts = wire_weaver_core::codegen::item_enum::enum_def(
                        item_enum,
                        args.ext.no_alloc,
                    );
                    codegen_ts.append_all(ts);

                    let ts = wire_weaver_core::codegen::item_enum::enum_serdes(
                        item_enum,
                        args.ext.no_alloc,
                    );
                    codegen_ts.append_all(ts);
                }
                Item::Const(item_const) => {
                    let ts = wire_weaver_core::codegen::item_const::const_def(item_const);
                    codegen_ts.append_all(ts);
                }
            }
        }

        for api_level in &module.api_levels {
            if args.ext.server {
                let ts = wire_weaver_core::codegen::api_server::server_dispatcher(
                    api_level,
                    args.ext.no_alloc,
                    args.ext.use_async,
                    &method_model,
                    &property_model,
                    &args.context_ident,
                );
                codegen_ts.append_all(ts);
            }

            if args.ext.client {
                let ts = wire_weaver_core::codegen::api_client::client(
                    api_level,
                    args.ext.no_alloc,
                    !args.ext.raw_client,
                    &args.context_ident,
                );
                codegen_ts.append_all(ts);
            }
        }
    }

    if !args.ext.debug_to_file.is_empty() {
        let ts_formatted = api::format_rust(format!("{codegen_ts}").as_str());
        let path = manifest_dir.join(&args.ext.debug_to_file);
        match File::create(&path) {
            Ok(mut f) => {
                if let Err(e) = f.write_all(ts_formatted.as_bytes()) {
                    eprintln!("Debug file write failed: {e:?}");
                }
            }
            Err(e) => {
                eprintln!("Debug file create failed: {path:?} {:?}", e);
            }
        }
    }

    codegen_ts
}

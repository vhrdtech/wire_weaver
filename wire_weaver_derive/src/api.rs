use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use darling::ast::NestedMeta;
use darling::{Error, FromMeta};
use pathsearch::find_executable_in_path;
use proc_macro2::TokenStream;
use quote::{TokenStreamExt, quote};
use relative_path::RelativePath;
use subprocess::{Exec, Redirection};
use syn::ItemMod;

use wire_weaver_core::ast::{Item, Source};
use wire_weaver_core::method_model::{MethodModel, MethodModelKind};
use wire_weaver_core::property_model::{PropertyModel, PropertyModelKind};
use wire_weaver_core::transform::Transform;

#[derive(Debug, FromMeta)]
struct Args {
    ww: Option<String>,
    #[darling(default)]
    client: bool,
    #[darling(default)]
    raw_client: bool,
    #[darling(default)]
    server: bool,
    no_alloc: bool,
    use_async: bool,
    #[darling(default)]
    debug_to_file: String,
    #[darling(default)]
    derive: String,
    #[darling(default)]
    method_model: String,
    #[darling(default)]
    property_model: String,
}

pub fn api(args: TokenStream, item: TokenStream) -> TokenStream {
    let attr_args = match NestedMeta::parse_meta_list(args) {
        Ok(v) => v,
        Err(e) => {
            return Error::from(e).write_errors();
        }
    };

    let args = match Args::from_list(&attr_args) {
        Ok(v) => v,
        Err(e) => {
            return e.write_errors();
        }
    };

    let mut api_mod: ItemMod = syn::parse2(item).expect("mod parse failed, probably syntax error");
    let api_mod_items = if let Some((_, items)) = &mut api_mod.content {
        items
    } else {
        panic!("mod cannot be empty, please provide Context struct");
    };

    let add_derives = args
        .derive
        .split(&[' ', ','])
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();

    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("env variable CARGO_MANIFEST_DIR should be set"),
    );
    if let Some(ww) = args.ww {
        let ww_path = if ww.starts_with("..") || ww.starts_with(".") {
            RelativePath::new(ww.as_str())
                .to_path(&manifest_dir)
                .to_str()
                .expect("path to user ww file is not valid Unicode")
                .to_owned()
        } else {
            ww.clone()
        };
        // eprintln!("path: {ww_path}");

        let mut transform = Transform::new();
        transform
            .load_and_push(Source::File { path: ww_path })
            .expect("ww load failed");

        let cx = transform
            .transform(&add_derives, false)
            .expect("ww transform failed");
        for (source, messages) in transform.messages() {
            for message in messages.messages() {
                println!("{:?} {:?}", source, message);
            }
        }

        let property_model = if args.property_model.is_empty() {
            PropertyModel {
                default: Some(PropertyModelKind::GetSet),
                items: vec![],
            }
        } else {
            PropertyModel::parse(&args.property_model).unwrap()
        };
        let method_model = if args.method_model.is_empty() {
            MethodModel {
                default: Some(MethodModelKind::Immediate),
                items: vec![],
            }
        } else {
            MethodModel::parse(&args.method_model).unwrap()
        };
        // let api_model_location = args
        //     .api_model
        //     .as_ref()
        //     .map(|a| syn::Path::from_string(a.as_str()).unwrap());
        let mut codegen_ts = TokenStream::new();
        for module in &cx.modules {
            for item in &module.items {
                match item {
                    Item::Struct(item_struct) => {
                        let ts = wire_weaver_core::codegen::item_struct::struct_def(
                            item_struct,
                            args.no_alloc,
                        );
                        codegen_ts.append_all(ts);

                        let ts = wire_weaver_core::codegen::item_struct::struct_serdes(
                            item_struct,
                            args.no_alloc,
                        );
                        codegen_ts.append_all(ts);
                    }
                    Item::Enum(item_enum) => {
                        let ts = wire_weaver_core::codegen::item_enum::enum_def(
                            item_enum,
                            args.no_alloc,
                        );
                        codegen_ts.append_all(ts);

                        let ts = wire_weaver_core::codegen::item_enum::enum_serdes(
                            item_enum,
                            args.no_alloc,
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
                // TODO: key on a provided API entry point
                // TODO: Modify Context and/or Client structs accordingly
                if args.server {
                    let ts = wire_weaver_core::codegen::api_server::server_dispatcher(
                        api_level,
                        args.no_alloc,
                        args.use_async,
                        &method_model,
                        &property_model,
                    );
                    codegen_ts.append_all(ts);
                }

                if args.client {
                    let ts = wire_weaver_core::codegen::api_client::client(
                        api_level,
                        args.no_alloc,
                        !args.raw_client,
                    );
                    codegen_ts.append_all(ts);
                }
            }
        }
        let items: syn::File =
            syn::parse2(codegen_ts).expect("internal: codegen_ts contains errors");
        for item in items.items {
            api_mod_items.push(item);
        }
    }

    let ts: TokenStream = quote! { #api_mod };

    if !args.debug_to_file.is_empty() {
        let ts_formatted = format_rust(format!("{ts}").as_str());
        //let manifest_dir_path = &PathBuf::from(manifest_dir);
        let path = manifest_dir.join(&args.debug_to_file);
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

    ts
}

pub fn format_rust(code: &str) -> String {
    let Some(rustfmt_path) = find_executable_in_path("rustfmt") else {
        println!("cargo:warning=rustfmt not found in PATH, skipping formatting");
        return code.to_string();
    };
    let Ok(rustfmt_run) = Exec::cmd(rustfmt_path)
        .args(&["--edition", "2021"])
        .stdin(code)
        .stdout(Redirection::Pipe)
        .capture()
    else {
        println!("cargo:warning=rustfmt failed, wrong code?");
        return code.to_string();
    };
    if !rustfmt_run.exit_status.success() {
        println!("cargo:warning=rustfmt failed, wrong code?");
        return code.to_string();
    }
    rustfmt_run.stdout_str()
}

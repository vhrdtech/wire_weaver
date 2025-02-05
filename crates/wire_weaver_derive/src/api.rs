use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use darling::ast::NestedMeta;
use darling::{Error, FromMeta};
use pathsearch::find_executable_in_path;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{quote, TokenStreamExt};
use subprocess::{Exec, Redirection};
use syn::ItemMod;

use wire_weaver_core::ast::{Item, Source};
use wire_weaver_core::transform::Transform;

#[derive(Debug, FromMeta)]
struct Args {
    ww: String,
    api_model: String,
    #[darling(default)]
    client: bool,
    #[darling(default)]
    server: bool,
    no_alloc: bool,
    #[darling(default)]
    skip_api_model_codegen: bool,
    #[darling(default)]
    debug_to_file: String,
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

    let mut api_mod: ItemMod = syn::parse2(item).unwrap();
    let api_mod_items = if let Some((_, items)) = &mut api_mod.content {
        items
    } else {
        panic!("mod cannot be empty, please provide Context struct");
    };

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let ww_path = if let Some(path) = args.ww.strip_prefix("..") {
        format!("{manifest_dir}/../{path}")
    } else if let Some(path) = args.ww.strip_prefix('.') {
        format!("{manifest_dir}/{path}")
    } else {
        args.ww.clone()
    };

    let mut transform = Transform::new();
    transform
        .load_and_push(Source::File { path: ww_path })
        .unwrap();

    let cx = transform.transform().unwrap();
    for (source, messages) in transform.messages() {
        for message in messages.messages() {
            println!("cargo:warning={:?} {:?}", source, message);
        }
    }

    let api_model_location = syn::Path::from_string(args.api_model.as_str()).unwrap();
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
                    let ts =
                        wire_weaver_core::codegen::item_enum::enum_def(item_enum, args.no_alloc);
                    codegen_ts.append_all(ts);

                    let ts =
                        wire_weaver_core::codegen::item_enum::enum_serdes(item_enum, args.no_alloc);
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
                    &api_model_location,
                    args.no_alloc,
                );
                codegen_ts.append_all(ts);
            }

            if args.client {
                let ts = wire_weaver_core::codegen::api_client::client(
                    api_level,
                    &api_model_location,
                    args.no_alloc,
                );
                codegen_ts.append_all(ts);
            }
        }
    }
    let items: syn::File = syn::parse2(codegen_ts).unwrap();
    for item in items.items {
        api_mod_items.push(item);
    }

    // let mut ts = TokenStream::new();
    // ts.append_all(item);
    if !args.skip_api_model_codegen {
        let api_model: ItemMod =
            syn::parse2(generate_api_model(args.api_model.as_str(), args.no_alloc)).unwrap();
        api_mod_items.push(syn::Item::Mod(api_model));
    }

    let ts: TokenStream = quote! { #api_mod };

    if !args.debug_to_file.is_empty() {
        let ts_formatted = format_rust(format!("{ts}").as_str());
        let manifest_dir_path = &PathBuf::from(manifest_dir);
        let path = manifest_dir_path.join(&args.debug_to_file);
        match File::create(&path) {
            Ok(mut f) => {
                if let Err(e) = f.write_all(ts_formatted.as_bytes()) {
                    println!("cargo:warning=Debug file write failed: {e:?}");
                }
            }
            Err(e) => {
                println!("cargo:warning=Debug file create failed: {path:?} {:?}", e);
            }
        }
    }

    ts
}

fn generate_api_model(api_model: &str, no_alloc: bool) -> TokenStream {
    let mut transform = Transform::new();
    // TODO: use registry to fetch it
    let registry_dir = std::env::var("WW_REGISTRY_DIR").unwrap();
    transform
        .load_and_push(Source::File {
            path: format!("{registry_dir}/client_server_v0_1/client_server.ww"),
        })
        .unwrap();

    let cx = transform.transform().unwrap();
    for (source, messages) in transform.messages() {
        for message in messages.messages() {
            println!("cargo:warning={:?} {:?}", source, message);
        }
    }

    let ts = wire_weaver_core::codegen::generate(&cx, no_alloc);
    let api_model = Ident::new(api_model, Span::call_site());
    quote! {
        mod #api_model {
            #ts
        }
    }
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

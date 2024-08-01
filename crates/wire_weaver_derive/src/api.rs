use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use darling::ast::NestedMeta;
use darling::{Error, FromMeta};
use pathsearch::find_executable_in_path;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use subprocess::{Exec, Redirection};
use syn::{ItemImpl, ItemMod};

use wire_weaver_core::ast::Source;
use wire_weaver_core::transform::Transform;

#[derive(Debug, FromMeta)]
struct Args {
    ww: String,
    api_model: String,
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

    let mut transform = Transform::new();
    transform
        .load_and_push(Source::File { path: args.ww })
        .unwrap();

    let cx = transform.transform().unwrap();
    for (source, messages) in transform.messages() {
        for message in messages.messages() {
            println!("cargo:warning={:?} {:?}", source, message);
        }
    }

    for module in &cx.modules {
        for api_level in &module.api_levels {
            // TODO: key on a provided API entry point
            let location = syn::Path::from_string(args.api_model.as_str()).unwrap();
            let ts = wire_weaver_core::codegen::api::server_dispatcher(
                api_level,
                location,
                args.no_alloc,
            );
            let dispatcher: ItemImpl = syn::parse2(ts).unwrap();
            api_mod_items.push(syn::Item::Impl(dispatcher));
        }
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
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let ts_formatted = format_rust(format!("{ts}").as_str());
        let manifest_dir_path = &PathBuf::from(manifest_dir);
        File::create(manifest_dir_path.join(args.debug_to_file))
            .unwrap()
            .write_all(ts_formatted.as_bytes())
            .unwrap();
    }

    ts
}

fn generate_api_model(api_model: &str, no_alloc: bool) -> TokenStream {
    let mut transform = Transform::new();
    // TODO: use registry to fetch it
    transform
        .load_and_push(Source::File {
            path: "/Users/roman/git/wire_weaver_registry/client_server_v0_1/client_server.ww"
                .into(),
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

use crate::ww_impl_args::ApiArgs;
use convert_case::Casing;
use proc_macro2::{Span, TokenStream};
use quote::{TokenStreamExt, quote};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use syn::Ident;
use wire_weaver_core::codegen::api_client::GenClientConfigRaw;
use wire_weaver_core::codegen::api_server::GenServerConfigRaw;
use wire_weaver_core::load_dep;
use wire_weaver_core::method_model::{MethodModel, MethodModelKind};
use wire_weaver_core::property_model::{PropertyModel, PropertyModelKind};
use wire_weaver_core::{ClientModel, gen_client, gen_server};

pub fn ww_api(args: ApiArgs) -> TokenStream {
    api_inner(args).unwrap_or_else(|e| syn::Error::new(Span::call_site(), e).to_compile_error())
}

pub fn ww_impl(args: ApiArgs) -> TokenStream {
    api_inner(args).unwrap_or_else(|e| syn::Error::new(Span::call_site(), e).to_compile_error())
}

fn api_inner(args: ApiArgs) -> Result<TokenStream, String> {
    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("env variable CARGO_MANIFEST_DIR should be set"),
    );
    let api_bundle = load_dep(args.dep_name.to_string(), Some(args.trait_name.to_string()))
        .map_err(|e| format!("{e:?}"))?;

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

    // emit marker with correct spans to help IDEs navigate back to the source
    let dep_name = args.dep_name;
    let full_gid_const = Ident::new(
        format!("{}_FULL_GID", args.trait_name.to_string())
            .to_case(convert_case::Case::Constant)
            .as_str(),
        args.trait_name.span(),
    );
    let mut codegen_ts = quote! {
        pub use #dep_name :: #full_gid_const as _SOURCE_MARKER;
    };

    // generate server code if requested
    if args.ext.server {
        let ts = gen_server(
            &api_bundle,
            GenServerConfigRaw {
                no_alloc: args.ext.no_alloc,
                use_async: args.ext.use_async,
                method_model,
                property_model,
                server_struct_path: args.context_ident.clone(),
                generate_introspect: args.ext.introspect,
            },
        );
        codegen_ts.append_all(ts);
    }

    // generate client code if requested
    if !args.ext.client.is_empty() {
        let client = args.ext.client.split(&['+', ' ']).collect::<Vec<_>>();
        let mut usb_connect = false;
        let model = match client[0] {
            "raw" => ClientModel::Raw,
            "async_worker" | "full_client" => {
                for ext in &client[1..] {
                    usb_connect = *ext == "usb";
                }
                ClientModel::StdFullClient
            }
            "trait_client" => ClientModel::StdTraitClient,
            _ => {
                return Err(format!(
                    "client supports raw or async_worked modes, got: '{}'",
                    args.ext.client
                ));
            }
        };
        let ts = gen_client(
            &api_bundle,
            GenClientConfigRaw {
                model,
                client_struct_path: args.context_ident.clone(),
                usb_connect,
            },
        );
        codegen_ts.append_all(ts);
    }

    // save debug output to file if requested
    if !args.ext.debug_to_file.is_empty() {
        let path = manifest_dir.join(&args.ext.debug_to_file);
        if let Some(p) = path.parent()
            && !p.exists()
        {
            _ = std::fs::create_dir_all(p);
        }
        match File::create(&path) {
            Ok(mut f) => {
                // let level_debug = format!("{:#?}", &level);
                // for line in level_debug.split('\n') {
                //     f.write_fmt(format_args!("// {line}\n"))
                //         .map_err(|e| e.to_string())?;
                // }
                let ts_formatted = crate::util::format_rust(format!("{codegen_ts}").as_str());
                f.write_all(ts_formatted.as_bytes())
                    .map_err(|e| e.to_string())?;
            }
            Err(e) => {
                eprintln!("Debug file create failed: {path:?} {:?}", e);
            }
        }
    }

    Ok(codegen_ts)
}

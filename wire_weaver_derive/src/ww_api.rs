use crate::ww_impl_args::ApiArgs;
use proc_macro2::{Span, TokenStream};
use quote::{TokenStreamExt, quote};
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use wire_weaver_core::codegen::api_client::ClientModel;
use wire_weaver_core::codegen::introspect::introspect;
use wire_weaver_core::method_model::{MethodModel, MethodModelKind};
use wire_weaver_core::property_model::{PropertyModel, PropertyModelKind};
use wire_weaver_core::transform::load::load_api_level_recursive;

pub fn ww_api(args: ApiArgs) -> TokenStream {
    api_inner(args).unwrap_or_else(|e| syn::Error::new(Span::call_site(), e).to_compile_error())
}

pub fn ww_impl(args: ApiArgs) -> TokenStream {
    api_inner(args).unwrap_or_else(|e| syn::Error::new(Span::call_site(), e).to_compile_error())
}

fn api_inner(args: ApiArgs) -> Result<TokenStream, String> {
    let mut cache = HashMap::new();
    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("env variable CARGO_MANIFEST_DIR should be set"),
    );
    let mut level = load_api_level_recursive(
        &args.location,
        Some(args.trait_name.clone()),
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
        let ts = wire_weaver_core::codegen::api_client::client(
            &level,
            model,
            &args.context_ident,
            usb_connect,
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

    if args.ext.introspect {
        let ww_self_bytes_const = introspect(&level);
        codegen_ts.append_all(quote! {
            pub const INTROSPECT_BYTES: #ww_self_bytes_const;
        });
    }

    Ok(codegen_ts)
}

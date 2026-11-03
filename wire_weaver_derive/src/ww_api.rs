use crate::ww_impl_args::ApiArgs;
use convert_case::Casing;
use proc_macro2::{Span, TokenStream};
use quote::{TokenStreamExt, quote};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use syn::Ident;
use wire_weaver_core::codegen::api_client::GenClientConfigRaw;
use wire_weaver_core::codegen::api_server::{GenServerConfigRaw, IntrospectMode};
use wire_weaver_core::codegen::local_trait::{gen_local_trait, gen_local_trait_client_impl};
use wire_weaver_core::const_model::ConstModel;
use wire_weaver_core::handler_model::HandlerModel;
use wire_weaver_core::method_model::{MethodModel, MethodModelKind};
use wire_weaver_core::property_model::{PropertyModel, PropertyModelKind};
use wire_weaver_core::{ClientModel, gen_client, gen_server};
use wire_weaver_core::{CratePaths, load, load_dep_with_paths};

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
    let own_crate = args.dep_name == "crate";
    let (api_bundle, crate_paths) = if own_crate {
        // the API crate itself (local_trait, CLT-57): its types are named through `crate::`
        let bundle = load(&manifest_dir, Some(args.trait_name.to_string()), false)
            .map_err(|e| format!("{e:?}"))?;
        let mut paths = CratePaths::default();
        let name = bundle
            .root
            .crate_name(&bundle)
            .map_err(|e| format!("{e:?}"))?
            .to_string();
        paths.insert(name, vec!["crate".to_string()]);
        (bundle, paths)
    } else {
        load_dep_with_paths(args.dep_name.to_string(), Some(args.trait_name.to_string()))
            .map_err(|e| format!("{e:?}"))?
    };
    if own_crate && (args.ext.server || !args.ext.client.is_empty()) {
        return Err("`crate :: Trait` only generates `local_trait`, server and client code is generated from \
                    another crate depending on the API crate"
            .into());
    }
    let context_ident = || {
        args.context_ident.clone().ok_or_else(|| {
            "server and client code need `for <struct>`: ww_codegen!(api :: Trait for Struct, ..)".to_string()
        })
    };

    let property_model = if args.ext.property_model.is_empty() {
        PropertyModel {
            default: Some(PropertyModelKind::GetSet),
            items: vec![],
        }
    } else {
        PropertyModel::parse(&args.ext.property_model)
            .map_err(|e| format!("failed to parse property model: {e}"))?
    };
    let const_model = ConstModel::parse(&args.ext.const_model)
        .map_err(|e| format!("failed to parse const model: {e}"))?;
    let handler_model = HandlerModel::parse(&args.ext.handler_model)
        .map_err(|e| format!("failed to parse handler model: {e}"))?;
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
        format!("{}_FULL_GID", args.trait_name)
            .to_case(convert_case::Case::Constant)
            .as_str(),
        args.trait_name.span(),
    );
    let mut codegen_ts = if own_crate {
        quote! {}
    } else {
        quote! {
            // unused when the call comes from a `macro_rules!` whose module is not reachable from outside
            #[allow(unused_imports)]
            pub use #dep_name :: #full_gid_const as _SOURCE_MARKER;
        }
    };

    if !args.ext.local_trait.is_empty() {
        let ident = syn::parse_str::<Ident>(&args.ext.local_trait)
            .map_err(|e| format!("local_trait must be a trait name: {e}"))?;
        codegen_ts.append_all(gen_local_trait(&api_bundle, &crate_paths, &ident));
    }

    // generate server code if requested
    if args.ext.server {
        let introspect_mode = match args.ext.introspect.as_str() {
            "disabled" => IntrospectMode::Disabled,
            "" => IntrospectMode::Disabled,
            "no_docs" => IntrospectMode::NoDocs,
            "with_docs" => IntrospectMode::WithDocs,
            u => {
                return Err(format!(
                    "Unknown introspect mode '{u}', supported: \"disabled\" or \"\", \"no_docs\" and \"with_docs\""
                ));
            }
        };
        let medium = if args.ext.medium.is_empty() {
            None
        } else {
            Some(
                syn::parse_str::<syn::Path>(&args.ext.medium)
                    .map_err(|e| format!("failed to parse medium type path: {e}"))?,
            )
        };
        let ts = gen_server(
            &api_bundle,
            GenServerConfigRaw {
                no_alloc: args.ext.no_alloc,
                use_async: args.ext.use_async,
                handler_model,
                method_model,
                property_model,
                const_model,
                server_struct_path: context_ident()?,
                introspect_mode,
                multi_req: args.ext.multi_req,
                medium,
                omit: args.ext.omit.clone(),
                observe_batch: (args.ext.observe_batch != 0).then_some(args.ext.observe_batch),
                observe_outbox: (args.ext.observe_outbox != 0).then_some(args.ext.observe_outbox),
                observe_cells: args
                    .ext
                    .observe_cells
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
                crate_paths: crate_paths.clone(),
            },
        );
        codegen_ts.append_all(ts);
    }

    // generate client code if requested
    if !args.ext.client.is_empty() {
        let client = args.ext.client.split(&['+', ' ']).collect::<Vec<_>>();
        let model = match client[0] {
            "raw" => ClientModel::Raw,
            "std_client" => ClientModel::StdFullClient,
            "trait_client" => ClientModel::StdTraitClient,
            _ => {
                return Err(format!(
                    "client must be std_client, trait_client or raw, got: '{}'",
                    args.ext.client
                ));
            }
        };
        let ts = gen_client(
            &api_bundle,
            GenClientConfigRaw {
                model,
                client_struct_path: context_ident()?,
                crate_paths: crate_paths.clone(),
            },
        );
        codegen_ts.append_all(ts);
        if !args.ext.impl_local_trait.is_empty() {
            let trait_path = syn::parse_str::<syn::Path>(&args.ext.impl_local_trait)
                .map_err(|e| format!("impl_local_trait must be a trait path: {e}"))?;
            codegen_ts.append_all(gen_local_trait_client_impl(
                &api_bundle,
                &crate_paths,
                &trait_path,
                &context_ident()?,
            ));
        }
    } else if !args.ext.impl_local_trait.is_empty() {
        return Err(
            "impl_local_trait is implemented on the generated client: add client = \"std_client\""
                .into(),
        );
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
                let ts_formatted = crate::util::format_rust(format!("{codegen_ts}"));
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

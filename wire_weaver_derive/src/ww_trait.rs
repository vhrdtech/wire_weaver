use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream, TokenTree};
use quote::{ToTokens, quote};
use std::path::PathBuf;
use syn::spanned::Spanned;
use syn::{
    Attribute, Error, Expr, FnArg, GenericArgument, ItemTrait, Lifetime, Lit, Meta, PathArguments,
    PathSegment, ReturnType, TraitItem, Type, Visibility, parse2,
};
use wire_weaver_core::internal::ww_self::ApiBundleOwned;
use wire_weaver_core::internal::ww_self::errors::error_list_docs;
use wire_weaver_core::internal::{
    ErrorListError, PropertyAccess, PropertyMacroArgs, StreamAndImplMacroArgs,
};
use wire_weaver_core::load;

pub fn ww_trait(attr: TokenStream, item: TokenStream) -> TokenStream {
    ww_trait_inner(attr, item).unwrap_or_else(|e| e.to_compile_error())
}

fn ww_trait_inner(attr: TokenStream, item: TokenStream) -> Result<TokenStream, Error> {
    let item_trait: ItemTrait = syn::parse2(item)?;
    let trait_name = item_trait.ident.to_string();
    let crate_path = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("env variable CARGO_MANIFEST_DIR should be set"),
    );
    // Parse fully to catch errors early - in API crates, not in user ones that depend on them.
    let api_bundle = load(&crate_path, Some(trait_name.clone()), false).map_err(|e| {
        // a wrong #[errors(..)] list points at the name in it
        match e.chain().find_map(|e| e.downcast_ref::<ErrorListError>()) {
            Some(list_e) => Error::new(
                error_list_span(&item_trait, list_e).unwrap_or_else(Span::call_site),
                list_e.to_string(),
            ),
            None => Error::new(Span::call_site(), format!("{e:?}")),
        }
    })?;

    let doc_trait =
        doc_trait(&item_trait, Some(&api_bundle)).map_err(|e| Error::new(Span::call_site(), e))?;
    let full_gid = Ident::new(
        format!("{}_FULL_GID", item_trait.ident)
            .to_case(Case::Constant)
            .as_str(),
        item_trait.ident.span(),
    );
    let compact_gid = Ident::new(
        format!("{}_COMPACT_GID", item_trait.ident)
            .to_case(Case::Constant)
            .as_str(),
        item_trait.ident.span(),
    );
    let full_gid_doc = format!("Crate name and full version of the [{trait_name}] API.");
    let compact_gid_doc = format!(
        "Global ID and version of the [{trait_name}] API, if it has one in the registry, shorter on the wire than [{full_gid}]."
    );
    Ok(quote! {
        #doc_trait
        #[doc = #full_gid_doc]
        pub const #full_gid: ww_version::FullVersion = wire_weaver::full_version!();
        #[doc = #compact_gid_doc]
        pub const #compact_gid: Option<ww_version::CompactVersion> = wire_weaver::compact_version!(#attr);
    })
}

/// Rust trait of the same name as the API trait, so that rustdoc shows the API, see `wire_weaver::api_doc`.
///
/// Resources keep their original spans, so the compiler checks their types in the defining crate (missing
/// lifetimes, unknown types or traits), and IDEs can go to their definitions and show them on hover.
fn doc_trait(
    item_trait: &ItemTrait,
    api_bundle: Option<&ApiBundleOwned>,
) -> Result<TokenStream, String> {
    let mut items = TokenStream::new();
    for item in &item_trait.items {
        match item {
            TraitItem::Fn(item_fn) => {
                let docs = doc_attrs(&item_fn.attrs);
                let errors = api_bundle
                    .map(|bundle| errors_section(bundle, &item_fn.sig.ident.to_string()))
                    .unwrap_or_default();
                let since = since_note(&item_fn.attrs);
                let ident = &item_fn.sig.ident;
                let inputs = &item_fn.sig.inputs;
                let output = &item_fn.sig.output;
                let mut lifetimes = vec![];
                for arg in inputs {
                    if let FnArg::Typed(pat_type) = arg {
                        collect_lifetimes(&pat_type.ty, &mut lifetimes);
                    }
                }
                if let ReturnType::Type(_, ty) = output {
                    collect_lifetimes(ty, &mut lifetimes);
                }
                let lifetimes = generic_lifetimes(lifetimes);
                let stream = stream_fn_note(&item_fn.attrs)?;
                let notes = notes([stream, since]);
                items.extend(quote! {
                    #(#docs)*
                    #(#[doc = #errors])*
                    #notes
                    fn #ident #lifetimes (#inputs) #output;
                });
            }
            TraitItem::Macro(item_macro) => {
                let Some(kind) = item_macro.mac.path.get_ident().map(|i| i.to_string()) else {
                    continue;
                };
                let docs = doc_attrs(&item_macro.attrs);
                let since = since_note(&item_macro.attrs);
                let tokens = item_macro.mac.tokens.clone();
                let errors_of = |name: &Ident| {
                    api_bundle
                        .map(|bundle| errors_section(bundle, &name.to_string()))
                        .unwrap_or_default()
                };
                match kind.as_str() {
                    "stream" | "sink" => {
                        let args: StreamAndImplMacroArgs =
                            parse2(tokens).map_err(|e| format!("{kind}!: {e}"))?;
                        let ident = &args.resource_name;
                        let ty = &args.type_or_trait;
                        let mut lifetimes = vec![];
                        collect_lifetimes(ty, &mut lifetimes);
                        let lifetimes = generic_lifetimes(lifetimes);
                        let (index, array) = index_arg(&args.multiplicity);
                        let (marker, note) = if kind == "stream" {
                            (quote! { Stream }, "Stream from the device to a client.")
                        } else {
                            (quote! { Sink }, "Sink from a client to the device.")
                        };
                        let credit = item_macro
                            .attrs
                            .iter()
                            .any(|a| a.path().is_ident("credit"))
                            .then(|| "Credit based flow control (`#[credit]`).".to_string());
                        let delivery = item_macro.attrs.iter().find_map(|a| {
                            if a.path().is_ident("counted") {
                                Some("Counted: every event has a position, losses are seen exactly (`#[counted]`).".to_string())
                            } else if a.path().is_ident("reliable") {
                                Some("Reliable: acknowledged, resent and kept on the device until received (`#[reliable]`).".to_string())
                            } else {
                                None
                            }
                        });
                        let batched = item_macro
                            .attrs
                            .iter()
                            .any(|a| a.path().is_ident("batched"))
                            .then(|| "Batched: items are sent in batches, one `Series` per event (`#[batched]`).".to_string());
                        let notes = notes([
                            Some(note.to_string()),
                            array,
                            credit,
                            delivery,
                            batched,
                            since,
                        ]);
                        items.extend(quote! {
                            #(#docs)*
                            #notes
                            fn #ident #lifetimes (#index) -> wire_weaver::api_doc::#marker<#ty>;
                        });
                    }
                    "property" => {
                        let args: PropertyMacroArgs =
                            parse2(tokens).map_err(|e| format!("property!: {e}"))?;
                        let ident = &args.resource_name;
                        let ty = &args.ty;
                        let mut lifetimes = vec![];
                        collect_lifetimes(ty, &mut lifetimes);
                        let err_ty = args.write_err_ty.as_ref().map(|err_ty| {
                            collect_lifetimes(&Type::Path(err_ty.clone()), &mut lifetimes);
                            quote! { , #err_ty }
                        });
                        let lifetimes = generic_lifetimes(lifetimes);
                        let (index, array) = index_arg(&args.multiplicity);
                        let access = match args.access {
                            PropertyAccess::Const => "Constant property, never changes.",
                            PropertyAccess::ReadOnly { observe: false, .. } => {
                                "Read-only property."
                            }
                            PropertyAccess::ReadOnly { batched: true, .. } => {
                                "Read-only property, changes and every sample can be observed."
                            }
                            PropertyAccess::ReadOnly { observe: true, .. } => {
                                "Read-only property, changes can be observed."
                            }
                            PropertyAccess::ReadWrite { observe: false, .. } => {
                                "Read-write property."
                            }
                            PropertyAccess::ReadWrite { batched: true, .. } => {
                                "Read-write property, changes and every sample can be observed."
                            }
                            PropertyAccess::ReadWrite { observe: true, .. } => {
                                "Read-write property, changes can be observed."
                            }
                            PropertyAccess::WriteOnly => "Write-only property.",
                        };
                        let notes = notes([Some(access.to_string()), array, since]);
                        let errors = errors_of(ident);
                        items.extend(quote! {
                            #(#docs)*
                            #(#[doc = #errors])*
                            #notes
                            fn #ident #lifetimes (#index) -> wire_weaver::api_doc::Property<#ty #err_ty>;
                        });
                    }
                    "ww_impl" => {
                        let args: StreamAndImplMacroArgs =
                            parse2(tokens).map_err(|e| format!("ww_impl!: {e}"))?;
                        let ident = &args.resource_name;
                        let ext_trait = &args.type_or_trait;
                        let (index, array) = index_arg(&args.multiplicity);
                        let notes = notes([array, since]);
                        items.extend(quote! {
                            #(#docs)*
                            #notes
                            fn #ident (#index) -> impl #ext_trait;
                        });
                    }
                    // reserved!(..) only takes up resource ids, nothing to show
                    _ => {}
                }
            }
            TraitItem::Const(item_const) => {
                let docs = doc_attrs(&item_const.attrs);
                let since = since_note(&item_const.attrs);
                let ident = &item_const.ident;
                let ty = &item_const.ty;
                let mut lifetimes = vec![];
                collect_lifetimes(ty, &mut lifetimes);
                let lifetimes = generic_lifetimes(lifetimes);
                let notes = notes([Some("Constant property, never changes.".to_string()), since]);
                items.extend(quote! {
                    #(#docs)*
                    #notes
                    #[allow(non_snake_case)]
                    fn #ident #lifetimes () -> wire_weaver::api_doc::Property<#ty>;
                });
            }
            _ => {}
        }
    }

    let docs = doc_attrs(&item_trait.attrs);
    let vis = &item_trait.vis;
    let ident = &item_trait.ident;
    let note = "WireWeaver API: describes the resources, it is not implemented directly, server and client code \
        come from `ww_codegen!`, see [wire_weaver::api_doc].";
    // a private trait is only used by codegen, which reads the source
    let allow_dead_code =
        (!matches!(vis, Visibility::Public(_))).then(|| quote! { #[allow(dead_code)] });
    Ok(quote! {
        #(#docs)*
        #[doc = ""]
        #[doc = #note]
        #allow_dead_code
        #vis trait #ident {
            #items
        }
    })
}

/// `# Errors` of a method, stream function or property with `#[errors(..)]` (API-32), each entry with its variant's
/// doc line, as in the generated client's docs.
fn errors_section(api_bundle: &ApiBundleOwned, item: &str) -> Vec<String> {
    let Some(kind) = api_bundle
        .root
        .items
        .iter()
        .find(|i| i.ident == item)
        .map(|i| &i.kind)
    else {
        return vec![];
    };
    let (Some(errors), Some(err_ty)) = (kind.error_list(), kind.err_ty(api_bundle)) else {
        return vec![];
    };
    let mut lines = vec![String::new(), " # Errors".to_string(), String::new()];
    for (name, doc) in error_list_docs(errors, err_ty, api_bundle) {
        lines.push(match doc {
            Some(doc) => format!(" - `{name}`: {doc}"),
            None => format!(" - `{name}`"),
        });
    }
    lines
}

/// Span of the name an [ErrorListError] is about, in the method's `#[errors(..)]`, or of the attribute itself.
fn error_list_span(item_trait: &ItemTrait, e: &ErrorListError) -> Option<Span> {
    let attrs = item_trait.items.iter().find_map(|item| match item {
        TraitItem::Fn(f) if f.sig.ident == e.method => Some(&f.attrs),
        TraitItem::Macro(m) => {
            let tokens = m.mac.tokens.to_string();
            tokens
                .contains(&e.method)
                .then_some(&m.attrs)
                .filter(|attrs| attrs.iter().any(|a| a.path().is_ident("errors")))
        }
        TraitItem::Const(c) if c.ident == e.method => Some(&c.attrs),
        _ => None,
    })?;
    let attr = attrs.iter().find(|a| a.path().is_ident("errors"))?;
    let name_span = e.name.as_ref().and_then(|name| {
        let Meta::List(list) = &attr.meta else {
            return None;
        };
        find_ident(list.tokens.clone(), name)
    });
    Some(name_span.unwrap_or_else(|| attr.path().span()))
}

fn find_ident(tokens: TokenStream, name: &str) -> Option<Span> {
    for tt in tokens {
        match tt {
            TokenTree::Ident(ident) if ident == name => return Some(ident.span()),
            TokenTree::Group(group) => {
                if let Some(span) = find_ident(group.stream(), name) {
                    return Some(span);
                }
            }
            _ => {}
        }
    }
    None
}

fn doc_attrs(attrs: &[Attribute]) -> Vec<&Attribute> {
    attrs.iter().filter(|a| a.path().is_ident("doc")).collect()
}

fn since_note(attrs: &[Attribute]) -> Option<String> {
    let attr = attrs.iter().find(|a| a.path().is_ident("since"))?;
    if let Meta::NameValue(name_value) = &attr.meta
        && let Expr::Lit(expr_lit) = &name_value.value
        && let Lit::Str(lit_str) = &expr_lit.lit
    {
        Some(format!("Since `{}`.", lit_str.value()))
    } else {
        None
    }
}

/// Note of a stream function (`#[stream(T, kind)] fn ..`): opening it starts a stream of `T`.
fn stream_fn_note(attrs: &[Attribute]) -> Result<Option<String>, String> {
    let Some(attr) = attrs.iter().find(|a| a.path().is_ident("stream")) else {
        return Ok(None);
    };
    let tokens = attr
        .meta
        .require_list()
        .map_err(|e| format!("#[stream(..)]: {e}"))?
        .tokens
        .to_string();
    // the item type, then the delivery and batching keywords (the type may have commas of its own)
    let mut ty = tokens.trim();
    let mut kind = "plain".to_string();
    let mut batched = None;
    while let Some((rest, last)) = ty.rsplit_once(',') {
        let last: String = last.split_whitespace().collect();
        match last.as_str() {
            "counted" | "reliable" | "plain" => kind = last,
            "batched" => batched = Some("batched"),
            "batched(untimed)" => batched = Some("batched, untimed"),
            "" => {}
            _ => break,
        }
        ty = rest.trim();
    }
    let batched = batched.map(|b| format!(" ({b})")).unwrap_or_default();
    Ok(Some(format!(
        "Stream function: calling it opens a {kind} stream of `{ty}`{batched} from the device, with this result."
    )))
}

/// Notes about a resource, added as the last paragraph of its documentation.
fn notes<const N: usize>(notes: [Option<String>; N]) -> TokenStream {
    let notes: Vec<_> = notes.into_iter().flatten().collect();
    if notes.is_empty() {
        return quote! {};
    }
    let notes = notes.join(" ");
    quote! {
        #[doc = ""]
        #[doc = #notes]
    }
}

/// `index: UNib32` (or the custom index type) for resource arrays, and a note about it.
fn index_arg(multiplicity: &Option<Option<PathSegment>>) -> (TokenStream, Option<String>) {
    match multiplicity {
        None => (quote! {}, None),
        Some(index_ty) => {
            let index_ty = index_ty
                .as_ref()
                .map(|ty| ty.to_token_stream())
                .unwrap_or_else(|| quote! { wire_weaver::shrink_wrap::UNib32 });
            (
                quote! { index: #index_ty },
                Some("Array of resources, addressed by `index`.".to_string()),
            )
        }
    }
}

/// `<'a, 'i>` to declare on a doc trait fn, `'static` and `'_` don't need declaring.
fn generic_lifetimes(mut lifetimes: Vec<Lifetime>) -> TokenStream {
    lifetimes.retain(|l| l.ident != "static" && l.ident != "_");
    lifetimes.sort_by(|a, b| a.ident.cmp(&b.ident));
    lifetimes.dedup_by(|a, b| a.ident == b.ident);
    if lifetimes.is_empty() {
        quote! {}
    } else {
        quote! { < #(#lifetimes),* > }
    }
}

fn collect_lifetimes(ty: &Type, lifetimes: &mut Vec<Lifetime>) {
    match ty {
        Type::Path(type_path) => {
            if let Some(last) = type_path.path.segments.last()
                && let PathArguments::AngleBracketed(angle) = &last.arguments
            {
                for arg in &angle.args {
                    match arg {
                        GenericArgument::Lifetime(lifetime) => {
                            lifetimes.push(lifetime.clone());
                        }
                        GenericArgument::Type(ty) => collect_lifetimes(ty, lifetimes),
                        _ => {}
                    }
                }
            }
        }
        Type::Reference(type_reference) => {
            if let Some(lifetime) = &type_reference.lifetime {
                lifetimes.push(lifetime.clone());
            }
            collect_lifetimes(&type_reference.elem, lifetimes);
        }
        Type::Slice(type_slice) => {
            collect_lifetimes(&type_slice.elem, lifetimes);
        }
        Type::Array(type_array) => {
            collect_lifetimes(&type_array.elem, lifetimes);
        }
        Type::Tuple(type_tuple) => {
            for ty in &type_tuple.elems {
                collect_lifetimes(ty, lifetimes);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_trait_of(src: &str) -> String {
        doc_trait(&syn::parse_str(src).unwrap(), None)
            .unwrap()
            .to_string()
    }

    #[test]
    fn docs_and_notes() {
        let out = doc_trait_of(
            r#"
            /// Motor
            pub trait Motor {
                /// Speed setpoint
                #[since = "0.2.0"]
                property!(rw+observe speed: u32, SpeedError);
                reserved!(1..=3);
                /// Encoder
                #[credit]
                sink!(phases[]: Phase<'i>);
                ww_impl!(brake: ww_brake::Brake);
                const LIMIT: u32;
            }
            "#,
        );
        let expected = quote! {
            #[doc = " Motor"]
            #[doc = ""]
            #[doc = "WireWeaver API: describes the resources, it is not implemented directly, server and client code come from `ww_codegen!`, see [wire_weaver::api_doc]."]
            pub trait Motor {
                #[doc = " Speed setpoint"]
                #[doc = ""]
                #[doc = "Read-write property, changes can be observed. Since `0.2.0`."]
                fn speed() -> wire_weaver::api_doc::Property<u32, SpeedError>;
                #[doc = " Encoder"]
                #[doc = ""]
                #[doc = "Sink from a client to the device. Array of resources, addressed by `index`. Credit based flow control (`#[credit]`)."]
                fn phases<'i>(index: wire_weaver::shrink_wrap::UNib32) -> wire_weaver::api_doc::Sink<Phase<'i> >;
                fn brake() -> impl ww_brake::Brake;
                #[doc = ""]
                #[doc = "Constant property, never changes."]
                #[allow(non_snake_case)]
                fn LIMIT() -> wire_weaver::api_doc::Property<u32>;
            }
        };
        assert_eq!(out, expected.to_string());
    }

    #[test]
    fn errors_section_from_the_list() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/methods_api");
        let bundle = load(&path, Some("Methods".into()), false).unwrap();
        assert_eq!(
            errors_section(&bundle, "listed_errors"),
            [
                "",
                " # Errors",
                "",
                " - `Common(Busy)`: Busy with another operation, try again later",
                " - `Common(InUse)`: In use by {by}",
                " - `Nack`: The sensor did not answer",
            ]
        );
        assert!(errors_section(&bundle, "fallible_absent").is_empty());
    }

    #[test]
    fn errors_section_of_stream_fn_and_property() {
        let tests = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests");
        let bundle = load(&tests.join("streams_reliable_api"), None, false).unwrap();
        assert_eq!(
            errors_section(&bundle, "samples"),
            ["", " # Errors", "", " - `BadRate`: Rate 0"]
        );
        let bundle = load(
            &tests.join("properties_api"),
            Some("Properties".into()),
            false,
        )
        .unwrap();
        assert_eq!(
            errors_section(&bundle, "mode"),
            [
                "",
                " # Errors",
                "",
                " - `TooBig`: Above 10",
                " - `NotSet`: Not set yet"
            ]
        );
    }

    #[test]
    fn private_trait_allows_dead_code() {
        let out = doc_trait_of("trait Private { fn m(); }");
        assert!(out.contains("allow (dead_code)"), "{out}");
    }
}

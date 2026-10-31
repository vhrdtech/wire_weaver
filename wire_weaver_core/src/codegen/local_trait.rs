//! A real async Rust trait per API (CLT-57): implemented by the generated std client and by any plain struct, so a
//! call site written once against it works with a local implementation (a direct call: no server, no
//! serialization) and with a remote device or service.
//!
//! `gen_local_trait` emits the trait (and its `dyn`-compatible companion) where `ww_codegen!(crate :: Api,
//! local_trait = "Name")` is invoked, usually in the API crate itself; `gen_local_trait_client_impl` implements it
//! on a generated client (`ww_codegen!(api :: Api for Client, client = "std_client", impl_local_trait = "..")`).
//!
//! Prototype scope: methods, property reads and writes and plain unbatched streams of the root level. Nested
//! levels, resource arrays, observing, sinks and counted, reliable or batched streams are left out and listed in the
//! trait's docs.
use crate::codegen::CodegenBundle;
use crate::codegen::ty_def::{TyPos, ty_def};
use crate::transform::CratePaths;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::Path;
use ww_self::{ApiItemKindOwned, Batching, Delivery, Multiplicity, PropertyAccess, TypeOwned};

/// One async function of the local trait.
struct LocalFn {
    ident: Ident,
    docs: Vec<String>,
    args: Vec<(Ident, TokenStream)>,
    /// Output of the future: always a `Result`, the remote call can fail on its way.
    output: TokenStream,
    /// Body of the client's implementation, `self` being the client: the inherent generated call, awaited.
    client_body: TokenStream,
}

/// The functions of the root level that the trait expresses, and the names of the items it leaves out.
fn local_fns(api_bundle: &CodegenBundle) -> (Vec<LocalFn>, Vec<String>) {
    let mut fns = vec![];
    let mut skipped = vec![];
    for item in &api_bundle.root.items {
        let ident = Ident::new(&item.ident, Span::call_site());
        if item.multiplicity != Multiplicity::Flat {
            skipped.push(format!("`{}` (resource array)", item.ident));
            continue;
        }
        match &item.kind {
            ApiItemKindOwned::Method {
                args, return_ty, ..
            } => {
                let args = args
                    .iter()
                    .map(|arg| {
                        (
                            Ident::new(&arg.ident, Span::call_site()),
                            ty_def(api_bundle, &arg.ty, true, TyPos::Arg).unwrap(),
                        )
                    })
                    .collect::<Vec<_>>();
                let output = call_output(api_bundle, return_ty.as_ref());
                let names = args.iter().map(|(n, _)| n);
                fns.push(LocalFn {
                    client_body: quote! { Self::#ident(self, #(#names),*).call().await },
                    ident,
                    docs: item.docs.clone(),
                    args,
                    output,
                });
            }
            ApiItemKindOwned::Property {
                access,
                ty,
                write_err_ty,
                ..
            } => {
                let ty = ty_def(api_bundle, ty, true, TyPos::Arg).unwrap();
                if let PropertyAccess::ReadOnly { observe: true }
                | PropertyAccess::ReadWrite { observe: true } = access
                {
                    skipped.push(format!("observing `{}`", item.ident));
                }
                if matches!(
                    access,
                    PropertyAccess::Const
                        | PropertyAccess::ReadOnly { .. }
                        | PropertyAccess::ReadWrite { .. }
                ) {
                    let read = Ident::new(&format!("read_{}", item.ident), Span::call_site());
                    fns.push(LocalFn {
                        client_body: quote! { Self::#read(self).read().await },
                        ident: read,
                        docs: item.docs.clone(),
                        args: vec![],
                        output: quote! { Result<#ty, wire_weaver_client::Error> },
                    });
                }
                if matches!(
                    access,
                    PropertyAccess::ReadWrite { .. } | PropertyAccess::WriteOnly
                ) {
                    let write = Ident::new(&format!("write_{}", item.ident), Span::call_site());
                    let err = match write_err_ty {
                        Some(e) => {
                            let e = ty_def(api_bundle, e, true, TyPos::Arg).unwrap();
                            quote! { wire_weaver_client::CallError<#e> }
                        }
                        None => quote! { wire_weaver_client::Error },
                    };
                    fns.push(LocalFn {
                        client_body: quote! { Self::#write(self, #ident).write().await },
                        ident: write,
                        docs: item.docs.clone(),
                        args: vec![(ident, ty)],
                        output: quote! { Result<(), #err> },
                    });
                }
            }
            ApiItemKindOwned::Stream { is_up: true, .. } | ApiItemKindOwned::StreamFn { .. } => {
                let stream = item.kind.as_stream_fn().expect("a stream");
                if stream.delivery != Delivery::Plain || stream.batched.is_some() {
                    let what = match (stream.delivery, stream.batched) {
                        (_, Some(Batching::Timed | Batching::Untimed)) => "batched stream",
                        (Delivery::Counted, _) => "counted stream",
                        (Delivery::Reliable, _) => "reliable stream",
                        _ => "stream",
                    };
                    skipped.push(format!("`{}` ({what})", item.ident));
                    continue;
                }
                let item_ty = if stream.ty.is_byte_slice(api_bundle).unwrap() {
                    quote! { wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned }
                } else {
                    ty_def(api_bundle, stream.ty, true, TyPos::Arg).unwrap()
                };
                let args = stream
                    .args
                    .iter()
                    .map(|arg| {
                        (
                            Ident::new(&arg.ident, Span::call_site()),
                            ty_def(api_bundle, &arg.ty, true, TyPos::Arg).unwrap(),
                        )
                    })
                    .collect::<Vec<_>>();
                let names = args.iter().map(|(n, _)| n);
                let handle = quote! { wire_weaver_client::local::ItemStream<#item_ty> };
                let (output, adapt) = match stream.return_ty.map(|t| t.get_in_line(api_bundle)) {
                    Some(Ok(TypeOwned::Result { ok_ty, err_ty })) => {
                        let ok_ty = ty_def(api_bundle, ok_ty, true, TyPos::Arg).unwrap();
                        let err_ty = ty_def(api_bundle, err_ty, true, TyPos::Arg).unwrap();
                        (
                            quote! { Result<Result<(#ok_ty, #handle), #err_ty>, wire_weaver_client::Error> },
                            quote! { .map(|r| r.map(|(a, s)| (a, s.into_item_stream()))) },
                        )
                    }
                    None => (
                        quote! { Result<#handle, wire_weaver_client::Error> },
                        quote! { .map(|s| s.into_item_stream()) },
                    ),
                    Some(_) => {
                        skipped.push(format!(
                            "`{}` (stream with a non-Result return)",
                            item.ident
                        ));
                        continue;
                    }
                };
                fns.push(LocalFn {
                    client_body: quote! { Self::#ident(self, #(#names),*).call().await #adapt },
                    ident,
                    docs: item.docs.clone(),
                    args,
                    output,
                });
            }
            ApiItemKindOwned::Stream { is_up: false, .. } | ApiItemKindOwned::FlowSink { .. } => {
                skipped.push(format!("`{}` (sink)", item.ident));
            }
            ApiItemKindOwned::Trait { .. } => {
                skipped.push(format!("`{}` (nested trait level)", item.ident));
            }
        }
    }
    (fns, skipped)
}

/// What a call of a method gives: `Result<T, CallError<E>>` for a method returning `Result<T, E>`, else
/// `Result<T, Error>`, the same as the generated client's `.call()`.
fn call_output(api_bundle: &CodegenBundle, return_ty: Option<&TypeOwned>) -> TokenStream {
    match return_ty {
        Some(return_ty) => match return_ty.get_in_line(api_bundle).unwrap() {
            TypeOwned::Result { ok_ty, err_ty } => {
                let ok_ty = ty_def(api_bundle, ok_ty, true, TyPos::Arg).unwrap();
                let err_ty = ty_def(api_bundle, err_ty, true, TyPos::Arg).unwrap();
                quote! { Result<#ok_ty, wire_weaver_client::CallError<#err_ty>> }
            }
            _ => {
                let ty = ty_def(api_bundle, return_ty, true, TyPos::Arg).unwrap();
                quote! { Result<#ty, wire_weaver_client::Error> }
            }
        },
        None => quote! { Result<(), wire_weaver_client::Error> },
    }
}

/// The `Dyn` companion of the trait named `trait_ident`.
fn dyn_ident(trait_ident: &Ident) -> Ident {
    Ident::new(&format!("{trait_ident}Dyn"), trait_ident.span())
}

/// The local trait `trait_ident` of the API's root level, its `dyn`-compatible companion `<trait_ident>Dyn` (boxed
/// futures, implemented for every implementor of the trait) and the trait's implementation for `Box` and `Arc` of
/// the companion, so a generic call site also takes a trait object.
pub fn gen_local_trait(
    api_bundle: &ww_self::ApiBundleOwned,
    crate_paths: &CratePaths,
    trait_ident: &Ident,
) -> TokenStream {
    let api_bundle = &CodegenBundle::new(api_bundle, crate_paths);
    let (fns, skipped) = local_fns(api_bundle);
    let dyn_ident = dyn_ident(trait_ident);
    let api_name = &api_bundle.root.trait_name;
    let mut trait_docs = vec![
        format!(
            " The `{api_name}` API as an async Rust trait: implemented by the generated client, and by a local type for direct calls (CLT-57)."
        ),
        format!(
            " A trait object is a `Box<dyn {dyn_ident}>` or `Arc<dyn {dyn_ident}>`, which implement this trait too."
        ),
    ];
    if !skipped.is_empty() {
        trait_docs.push(String::new());
        trait_docs.push(format!(
            " Not in this trait yet: {}; use the client for them.",
            skipped.join(", ")
        ));
    }

    let sig = |f: &LocalFn| {
        let ident = &f.ident;
        let args = f.args.iter().map(|(n, t)| quote! { #n: #t });
        quote! { #ident(&self, #(#args),*) }
    };
    let trait_fns = fns.iter().map(|f| {
        let sig = sig(f);
        let output = &f.output;
        let docs = f.docs.iter().map(|d| quote! { #[doc = #d] });
        quote! {
            #(#docs)*
            fn #sig -> impl ::core::future::Future<Output = #output> + Send;
        }
    });
    let dyn_fns = fns.iter().map(|f| {
        let sig = sig(f);
        let output = &f.output;
        let docs = f.docs.iter().map(|d| quote! { #[doc = #d] });
        quote! {
            #(#docs)*
            fn #sig -> wire_weaver_client::local::BoxFuture<'_, #output>;
        }
    });
    let blanket_fns = fns.iter().map(|f| {
        let sig = sig(f);
        let output = &f.output;
        let ident = &f.ident;
        let names = f.args.iter().map(|(n, _)| n);
        quote! {
            fn #sig -> wire_weaver_client::local::BoxFuture<'_, #output> {
                Box::pin(<T as #trait_ident>::#ident(self, #(#names),*))
            }
        }
    });
    let deref_fns = fns
        .iter()
        .map(|f| {
            let sig = sig(f);
            let output = &f.output;
            let ident = &f.ident;
            let names = f.args.iter().map(|(n, _)| n);
            quote! {
                fn #sig -> impl ::core::future::Future<Output = #output> + Send {
                    <dyn #dyn_ident as #dyn_ident>::#ident(&**self, #(#names),*)
                }
            }
        })
        .collect::<Vec<_>>();
    let trait_docs = trait_docs.iter().map(|d| quote! { #[doc = #d] });
    let dyn_doc = format!(
        " [{trait_ident}] with boxed futures, for trait objects: every implementor of [{trait_ident}] implements it."
    );
    quote! {
        #(#trait_docs)*
        pub trait #trait_ident: Send + Sync {
            #(#trait_fns)*
        }

        #[doc = #dyn_doc]
        pub trait #dyn_ident: Send + Sync {
            #(#dyn_fns)*
        }

        impl<T: #trait_ident> #dyn_ident for T {
            #(#blanket_fns)*
        }

        impl #trait_ident for ::std::boxed::Box<dyn #dyn_ident> {
            #(#deref_fns)*
        }

        impl #trait_ident for ::std::sync::Arc<dyn #dyn_ident> {
            #(#deref_fns)*
        }
    }
}

/// The local trait at `trait_path` (made by [gen_local_trait] from the same API) implemented on the generated std
/// client `client_struct_path`, each function calling the client's own generated one.
pub fn gen_local_trait_client_impl(
    api_bundle: &ww_self::ApiBundleOwned,
    crate_paths: &CratePaths,
    trait_path: &Path,
    client_struct_path: &Path,
) -> TokenStream {
    let api_bundle = &CodegenBundle::new(api_bundle, crate_paths);
    let (fns, _skipped) = local_fns(api_bundle);
    let impl_fns = fns.iter().map(|f| {
        let ident = &f.ident;
        let args = f.args.iter().map(|(n, t)| quote! { #n: #t });
        let output = &f.output;
        let body = &f.client_body;
        quote! {
            async fn #ident(&self, #(#args),*) -> #output {
                #body
            }
        }
    });
    quote! {
        impl #trait_path for #client_struct_path {
            #(#impl_fns)*
        }
    }
}

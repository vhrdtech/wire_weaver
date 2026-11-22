//! A real async Rust trait per API level (CLT-57, CLT-58): implemented by the generated std client and by any plain
//! struct, so a call site written once against it works with a local implementation (a direct call: no server, no
//! serialization) and with a remote device or service.
//!
//! `gen_local_trait` emits the root's trait (and its `dyn`-compatible companion) where `ww_codegen!(crate :: Api,
//! local_trait = "Name")` is invoked, usually in the API crate itself, with the trait of every nested level defined
//! in the same crate; `gen_local_trait_client_impl` implements them on the generated client's level structs
//! (`ww_codegen!(api :: Api for Client, client = "std_client", impl_local_trait = "..")`).
//!
//! A nested level is a trait of its own, named on its definition (`#[ww_trait(local = "Name")]`), and reached
//! through an accessor of its parent: `type Sub<'a>: Sub + 'a where Self: 'a; fn sub(&self) -> Self::Sub<'_>;`, with
//! the index and a `Result` for an array of levels. The trait of a level from another crate is that crate's, found
//! next to its `#[ww_trait]` (where the `*_FULL_GID` const is).
//!
//! With `local_trait_send = false` the traits, their companions and their streams drop `Send` (SRV-73): futures and
//! item streams may hold `Rc` / `RefCell` state, the companions' trait objects are `Box` and `Rc`.
//!
//! Left out, and listed in each trait's docs: observing, sinks, counted, reliable or batched streams and keyed
//! arrays.
use crate::codegen::CodegenBundle;
use crate::codegen::ty_def::{TyPos, ty_def, ty_def_by_idx};
use crate::transform::CratePaths;
use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use std::collections::BTreeSet;
use syn::Path;
use ww_self::{
    ApiItemKindOwned, ApiItemOwned, ApiLevelOwned, Batching, Delivery, Multiplicity,
    PropertyAccess, TypeOwned,
};

/// One async function of a level's local trait.
pub(crate) struct LocalFn {
    pub(crate) ident: Ident,
    docs: Vec<String>,
    /// The element's index of an item of a resource array, its type: the first argument after `cx`.
    pub(crate) index: Option<TokenStream>,
    pub(crate) args: Vec<(Ident, TokenStream)>,
    /// Output of the future: always a `Result`, the remote call can fail on its way.
    output: TokenStream,
    /// Body of the client's implementation, `self` being the level's client: the inherent generated call, awaited.
    client_body: TokenStream,
    /// What the function stands for, for a server forwarding to the trait (SRV-49).
    pub(crate) kind: LocalFnKind,
    /// Index of its item in the level.
    pub(crate) item: usize,
}

impl LocalFn {
    /// The arguments after `cx`, as written in a signature: the index first.
    fn arg_defs(&self) -> Vec<TokenStream> {
        self.index
            .iter()
            .map(|ty| quote! { index: #ty })
            .chain(self.args.iter().map(|(n, t)| quote! { #n: #t }))
            .collect()
    }

    /// The arguments after `cx`, as passed on.
    fn arg_names(&self) -> Vec<TokenStream> {
        self.index
            .iter()
            .map(|_| quote! { index })
            .chain(self.args.iter().map(|(n, _)| quote! { #n }))
            .collect()
    }
}

/// What a function of the local trait stands for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum LocalFnKind {
    Method,
    /// Read of a property, `read_<name>`.
    Read,
    /// Write of a property, `write_<name>`.
    Write,
    /// Open of a plain stream.
    Stream,
    /// The valid indices of a resource array, `<array>_valid_indices`.
    ValidIndices,
}

/// The accessor of a nested level: `fn sub(&self) -> Self::Sub<'_>`, or `fn unit(&self, index: u32) ->
/// Result<Self::Unit<'_>, Error>` for an array of levels.
pub(crate) struct Accessor<'b> {
    pub(crate) ident: Ident,
    /// The associated type of the level, `Sub`.
    pub(crate) assoc: Ident,
    docs: Vec<String>,
    /// The index type of an array of levels.
    pub(crate) index: Option<TokenStream>,
    /// Index of its item in the level.
    pub(crate) item: usize,
    pub(crate) level: &'b ApiLevelOwned,
}

/// What the local trait of one level has.
pub(crate) struct LocalLevel<'b> {
    pub(crate) level: &'b ApiLevelOwned,
    pub(crate) fns: Vec<LocalFn>,
    pub(crate) accessors: Vec<Accessor<'b>>,
    /// The items it leaves out, for its docs.
    skipped: Vec<String>,
}

/// How generated code names the local traits of an API's levels.
pub(crate) enum TraitPaths<'a> {
    /// Code generated with the traits themselves, in the API crate (`gen_local_trait`): the root's and its crate's
    /// levels by their names, the root named `root`.
    Own { root: &'a Ident },
    /// Code generated in another crate: the root's trait at `root`, the root crate's levels next to it.
    Ext { root: &'a Path },
}

impl TraitPaths<'_> {
    /// The local trait of `level`, `Dyn` its companion. Errors name a level without a local trait.
    pub(crate) fn path(
        &self,
        api_bundle: &CodegenBundle,
        level: &ApiLevelOwned,
        dyn_companion: bool,
    ) -> Result<TokenStream, String> {
        let suffix = if dyn_companion { "Dyn" } else { "" };
        if std::ptr::eq(level, &api_bundle.root) {
            return Ok(match self {
                TraitPaths::Own { root } => {
                    let ident = Ident::new(&format!("{root}{suffix}"), root.span());
                    quote! { #ident }
                }
                TraitPaths::Ext { root } => with_last(root, suffix),
            });
        }
        let name = local_name(api_bundle, level)?;
        let ident = Ident::new(&format!("{name}{suffix}"), Span::call_site());
        if level.crate_idx == api_bundle.root.crate_idx {
            return Ok(match self {
                TraitPaths::Own { .. } => quote! { #ident },
                TraitPaths::Ext { root } => {
                    let mut parent = (*root).clone();
                    parent.segments.pop();
                    if parent.segments.is_empty() {
                        quote! { #ident }
                    } else {
                        let segments = parent.segments.iter();
                        quote! { #(#segments)::* :: #ident }
                    }
                }
            });
        }
        let module = api_bundle.trait_module_path(level);
        Ok(quote! { #module :: #ident })
    }
}

/// `path` with `suffix` appended to its last segment's name.
fn with_last(path: &Path, suffix: &str) -> TokenStream {
    let mut path = path.clone();
    if let Some(last) = path.segments.last_mut() {
        last.ident = Ident::new(&format!("{}{suffix}", last.ident), last.ident.span());
    }
    quote! { #path }
}

/// The name of `level`'s local trait from `#[ww_trait(local = "..")]`.
fn local_name<'b>(api_bundle: &'b CodegenBundle, level: &ApiLevelOwned) -> Result<&'b str, String> {
    let crate_name = level
        .crate_name(api_bundle)
        .map_err(|e| format!("`{}`: {e}", level.trait_name))?;
    api_bundle
        .local_trait(crate_name, &level.trait_name)
        .ok_or_else(|| {
            format!(
                "the local trait reaches the level `{crate_name}::{}`: name its trait on the definition, \
                 `#[ww_trait(local = \"Name\")]`",
                level.trait_name
            )
        })
}

/// The index type of an item of `multiplicity`, as the client takes it: `u32`, or the array's own index type.
fn index_ty(api_bundle: &CodegenBundle, multiplicity: &Multiplicity) -> Option<TokenStream> {
    match multiplicity {
        Multiplicity::Flat => None,
        Multiplicity::Array {
            index_type_idx: None,
        } => Some(quote! { u32 }),
        Multiplicity::Array {
            index_type_idx: Some(idx),
        } => Some(ty_def_by_idx(api_bundle, idx.0, true, TyPos::Arg).expect("the index type")),
    }
}

/// The functions and accessors of `level`'s local trait, and the items it leaves out. `send`: the trait's futures
/// and streams are `Send`.
pub(crate) fn local_level<'b>(
    api_bundle: &'b CodegenBundle,
    level: &'b ApiLevelOwned,
    send: bool,
) -> Result<LocalLevel<'b>, String> {
    let mut out = LocalLevel {
        level,
        fns: vec![],
        accessors: vec![],
        skipped: vec![],
    };
    for (item_index, item) in level.items.iter().enumerate() {
        let index = index_ty(api_bundle, &item.multiplicity);
        let before = out.fns.len() + out.accessors.len();
        local_item(api_bundle, &mut out, item_index, item, index.clone(), send)?;
        let expressed = out.fns.len() + out.accessors.len() > before;
        if expressed && index.is_some() {
            let ident = Ident::new(&format!("{}_valid_indices", item.ident), Span::call_site());
            out.fns.push(LocalFn {
                client_body: quote! { Self::#ident(self).with_call_context(cx).read().await.map_err(::core::convert::Into::into) },
                ident,
                docs: vec![format!(" Valid indices of `{}[]`.", item.ident)],
                index: None,
                args: vec![],
                output: quote! { Result<wire_weaver::ValidIndicesOwned, wire_weaver::call::Error> },
                kind: LocalFnKind::ValidIndices,
                item: item_index,
            });
        }
    }
    Ok(out)
}

/// The functions or accessor of `item`, or its name in `out.skipped`.
fn local_item<'b>(
    api_bundle: &'b CodegenBundle,
    out: &mut LocalLevel<'b>,
    item_index: usize,
    item: &'b ApiItemOwned,
    index: Option<TokenStream>,
    send: bool,
) -> Result<(), String> {
    let ident = Ident::new(&item.ident, Span::call_site());
    // the client's calls of an array's element take the index first
    let client_index = index.iter().map(|_| quote! { index }).collect::<Vec<_>>();
    match &item.kind {
        ApiItemKindOwned::Method {
            args, return_ty, ..
        } => {
            let args = arg_types(api_bundle, args);
            let output = call_output(api_bundle, return_ty.as_ref());
            let names = args.iter().map(|(n, _)| n);
            out.fns.push(LocalFn {
                client_body: quote! { Self::#ident(self, #(#client_index,)* #(#names),*).with_call_context(cx).call().await.map_err(::core::convert::Into::into) },
                ident,
                docs: item.docs.clone(),
                index,
                args,
                output,
                kind: LocalFnKind::Method,
                item: item_index,
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
                out.skipped.push(format!("observing `{}`", item.ident));
            }
            if matches!(
                access,
                PropertyAccess::Const
                    | PropertyAccess::ReadOnly { .. }
                    | PropertyAccess::ReadWrite { .. }
            ) {
                let read = Ident::new(&format!("read_{}", item.ident), Span::call_site());
                out.fns.push(LocalFn {
                    client_body: quote! { Self::#read(self, #(#client_index),*).with_call_context(cx).read().await.map_err(::core::convert::Into::into) },
                    ident: read,
                    docs: item.docs.clone(),
                    index: index.clone(),
                    args: vec![],
                    output: quote! { Result<#ty, wire_weaver::call::Error> },
                    kind: LocalFnKind::Read,
                    item: item_index,
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
                        quote! { wire_weaver::call::CallError<#e> }
                    }
                    None => quote! { wire_weaver::call::Error },
                };
                out.fns.push(LocalFn {
                    client_body: quote! { Self::#write(self, #(#client_index,)* #ident).with_call_context(cx).write().await.map_err(::core::convert::Into::into) },
                    ident: write,
                    docs: item.docs.clone(),
                    index,
                    args: vec![(ident, ty)],
                    output: quote! { Result<(), #err> },
                    kind: LocalFnKind::Write,
                    item: item_index,
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
                out.skipped.push(format!("`{}` ({what})", item.ident));
                return Ok(());
            }
            let item_ty = if stream.ty.is_byte_slice(api_bundle).unwrap() {
                quote! { wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned }
            } else {
                ty_def(api_bundle, stream.ty, true, TyPos::Arg).unwrap()
            };
            let args = arg_types(api_bundle, stream.args);
            let names = args.iter().map(|(n, _)| n);
            let handle = if send {
                quote! { wire_weaver::call::ItemStream<#item_ty> }
            } else {
                quote! { wire_weaver::call::LocalItemStream<#item_ty> }
            };
            // the client's item stream is `Send`; a `LocalItemStream` takes it by coercion
            let into_handle = quote! { { let s: #handle = s.into_item_stream(); s } };
            let (output, adapt) = match stream.return_ty.map(|t| t.get_in_line(api_bundle)) {
                Some(Ok(TypeOwned::Result { ok_ty, err_ty })) => {
                    let ok_ty = ty_def(api_bundle, ok_ty, true, TyPos::Arg).unwrap();
                    let err_ty = ty_def(api_bundle, err_ty, true, TyPos::Arg).unwrap();
                    (
                        quote! { Result<Result<(#ok_ty, #handle), #err_ty>, wire_weaver::call::Error> },
                        quote! { .map(|r| r.map(|(a, s)| (a, #into_handle))) },
                    )
                }
                None => (
                    quote! { Result<#handle, wire_weaver::call::Error> },
                    quote! { .map(|s| #into_handle) },
                ),
                Some(_) => {
                    out.skipped.push(format!(
                        "`{}` (stream with a non-Result return)",
                        item.ident
                    ));
                    return Ok(());
                }
            };
            out.fns.push(LocalFn {
                client_body: quote! { Self::#ident(self, #(#client_index,)* #(#names),*).with_call_context(cx).call().await.map_err(wire_weaver::call::Error::from) #adapt },
                ident,
                docs: item.docs.clone(),
                index,
                args,
                output,
                kind: LocalFnKind::Stream,
                item: item_index,
            });
        }
        ApiItemKindOwned::Stream { is_up: false, .. } | ApiItemKindOwned::FlowSink { .. } => {
            out.skipped.push(format!("`{}` (sink)", item.ident));
        }
        ApiItemKindOwned::Trait { .. } => {
            let level = item.get_as_level(api_bundle).map_err(|e| {
                format!(
                    "`{}`: the local trait needs the level's definition: {e}",
                    item.ident
                )
            })?;
            out.accessors.push(Accessor {
                assoc: Ident::new(&item.ident.to_case(Case::Pascal), Span::call_site()),
                ident,
                docs: item.docs.clone(),
                index,
                item: item_index,
                level,
            });
        }
        ApiItemKindOwned::Keys { .. } => {
            out.skipped.push(format!("`{}` (keyed! companion)", item.ident));
        }
    }
    Ok(())
}

fn arg_types(
    api_bundle: &CodegenBundle,
    args: &[ww_self::ArgumentOwned],
) -> Vec<(Ident, TokenStream)> {
    args.iter()
        .map(|arg| {
            (
                Ident::new(&arg.ident, Span::call_site()),
                ty_def(api_bundle, &arg.ty, true, TyPos::Arg).unwrap(),
            )
        })
        .collect()
}

/// What a call of a method gives: `Result<T, CallError<E>>` for a method returning `Result<T, E>`, else
/// `Result<T, Error>`, the same as the generated client's `.call()`.
fn call_output(api_bundle: &CodegenBundle, return_ty: Option<&TypeOwned>) -> TokenStream {
    match return_ty {
        Some(return_ty) => match return_ty.get_in_line(api_bundle).unwrap() {
            TypeOwned::Result { ok_ty, err_ty } => {
                let ok_ty = ty_def(api_bundle, ok_ty, true, TyPos::Arg).unwrap();
                let err_ty = ty_def(api_bundle, err_ty, true, TyPos::Arg).unwrap();
                quote! { Result<#ok_ty, wire_weaver::call::CallError<#err_ty>> }
            }
            _ => {
                let ty = ty_def(api_bundle, return_ty, true, TyPos::Arg).unwrap();
                quote! { Result<#ty, wire_weaver::call::Error> }
            }
        },
        None => quote! { Result<(), wire_weaver::call::Error> },
    }
}

/// Every level of the API by its path from the root, depth first, the root first with an empty path: what a client
/// or a server reaches.
pub(crate) fn walk_levels<'b>(
    api_bundle: &'b CodegenBundle,
    send: bool,
) -> Result<Vec<(Vec<usize>, LocalLevel<'b>)>, String> {
    fn walk<'b>(
        api_bundle: &'b CodegenBundle,
        level: &'b ApiLevelOwned,
        path: Vec<usize>,
        send: bool,
        out: &mut Vec<(Vec<usize>, LocalLevel<'b>)>,
    ) -> Result<(), String> {
        let local = local_level(api_bundle, level, send)?;
        let children = local
            .accessors
            .iter()
            .map(|a| (a.item, a.level))
            .collect::<Vec<_>>();
        out.push((path.clone(), local));
        for (item, child) in children {
            let mut path = path.clone();
            path.push(item);
            walk(api_bundle, child, path, send, out)?;
        }
        Ok(())
    }
    let mut out = vec![];
    walk(api_bundle, &api_bundle.root, vec![], send, &mut out)?;
    Ok(out)
}

/// The levels whose traits are generated with the root's: the root and every level of the root's crate reached
/// through levels of that crate, each once.
fn own_levels<'b>(
    api_bundle: &'b CodegenBundle,
    send: bool,
) -> Result<Vec<LocalLevel<'b>>, String> {
    let mut seen = BTreeSet::new();
    let mut out = vec![];
    let mut queue = vec![&api_bundle.root];
    while let Some(level) = queue.pop() {
        if !seen.insert(level.trait_name.clone()) {
            continue;
        }
        let local = local_level(api_bundle, level, send)?;
        for a in &local.accessors {
            if a.level.crate_idx == api_bundle.root.crate_idx {
                queue.push(a.level);
            }
        }
        out.push(local);
    }
    Ok(out)
}

/// The tokens that differ between the `Send` form of a trait and the one without (SRV-73).
struct Form {
    supertraits: TokenStream,
    future_send: TokenStream,
    box_future: TokenStream,
    shared: TokenStream,
}

impl Form {
    fn new(send: bool) -> Self {
        if send {
            Form {
                supertraits: quote! { : Send + Sync },
                future_send: quote! { + Send },
                box_future: quote! { wire_weaver::call::BoxFuture },
                shared: quote! { ::std::sync::Arc },
            }
        } else {
            Form {
                supertraits: quote! {},
                future_send: quote! {},
                box_future: quote! { wire_weaver::call::LocalBoxFuture },
                shared: quote! { ::std::rc::Rc },
            }
        }
    }
}

/// The local trait `root` of the API's root level and those of the nested levels of its crate, each with its
/// `dyn`-compatible companion `<Name>Dyn` (boxed futures, implemented for every implementor of the trait), its
/// implementation for `Box` and `Arc` of the companion, so a generic call site also takes a trait object, and for `&T`
/// of an implementor, so a local parent hands out its children by reference. Without `send`, the traits, their
/// futures and their streams aren't `Send` and the shared trait object is an `Rc` (SRV-73).
pub fn gen_local_trait(
    api_bundle: &ww_self::ApiBundleOwned,
    crate_paths: &CratePaths,
    root: &Ident,
    send: bool,
) -> TokenStream {
    let api_bundle = &CodegenBundle::new(api_bundle, crate_paths);
    if let Ok(crate_name) = api_bundle.root.crate_name(api_bundle)
        && let Some(named) = api_bundle.local_trait(crate_name, &api_bundle.root.trait_name)
        && *root != named
    {
        let msg = format!(
            "local_trait = \"{root}\", but `{}` names its local trait `{named}` with #[ww_trait(local = ..)]",
            api_bundle.root.trait_name
        );
        return quote! { compile_error!(#msg); };
    }
    let paths = TraitPaths::Own { root };
    let levels = match own_levels(api_bundle, send) {
        Ok(levels) => levels,
        Err(e) => return quote! { compile_error!(#e); },
    };
    let mut ts = TokenStream::new();
    for level in &levels {
        match gen_level_trait(api_bundle, &paths, level, send) {
            Ok(t) => ts.extend(t),
            Err(e) => ts.extend(quote! { compile_error!(#e); }),
        }
    }
    ts
}

/// The local trait of one level, its companion and their implementations.
fn gen_level_trait(
    api_bundle: &CodegenBundle,
    paths: &TraitPaths<'_>,
    local: &LocalLevel<'_>,
    send: bool,
) -> Result<TokenStream, String> {
    let trait_path = paths.path(api_bundle, local.level, false)?;
    let dyn_path = paths.path(api_bundle, local.level, true)?;
    let trait_ident = &trait_path;
    let dyn_ident = &dyn_path;
    let Form {
        supertraits,
        future_send,
        box_future,
        shared,
    } = Form::new(send);
    let api_name = &local.level.trait_name;
    let trait_name = trait_path.to_string().replace(' ', "");
    let dyn_name = dyn_path.to_string().replace(' ', "");
    let mut trait_docs = vec![format!(
        " The `{api_name}` API as an async Rust trait: implemented by the generated client, and by a local type for direct calls (CLT-57)."
    )];
    if send {
        trait_docs.push(format!(
            " A trait object is a `Box<dyn {dyn_name}>` or `Arc<dyn {dyn_name}>`, which implement this trait too, as does `&T` of an implementor."
        ));
    } else {
        trait_docs.push(format!(
            " Not `Send` (SRV-73): its futures and streams may hold `Rc` / `RefCell` state. A trait object is a `Box<dyn {dyn_name}>` or `Rc<dyn {dyn_name}>`, which implement this trait too, as does `&T` of an implementor."
        ));
    }
    if !local.skipped.is_empty() {
        trait_docs.push(String::new());
        trait_docs.push(format!(
            " Not in this trait yet: {}; use the client for them.",
            local.skipped.join(", ")
        ));
    }

    let mut trait_items = vec![];
    let mut dyn_items = vec![];
    let mut blanket_items = vec![];
    let mut deref_items = vec![];
    let mut ref_items = vec![];
    for a in &local.accessors {
        let child = paths.path(api_bundle, a.level, false)?;
        let child_dyn = paths.path(api_bundle, a.level, true)?;
        let (ident, assoc) = (&a.ident, &a.assoc);
        let docs = a.docs.iter().map(|d| quote! { #[doc = #d] }).collect::<Vec<_>>();
        let assoc_doc = format!(" The level `{}` hands out.", a.ident);
        let (index_def, index_name) = match &a.index {
            Some(ty) => (quote! { , index: #ty }, quote! { , index }),
            None => (quote! {}, quote! {}),
        };
        let wrap = |t: TokenStream| match &a.index {
            Some(_) => quote! { Result<#t, wire_weaver::call::Error> },
            None => t,
        };
        let output = wrap(quote! { Self::#assoc<'_> });
        let dyn_output = wrap(quote! { ::std::boxed::Box<dyn #child_dyn + '_> });
        let boxed = match &a.index {
            Some(_) => quote! {
                <T as #trait_ident>::#ident(self #index_name).map(|l| -> ::std::boxed::Box<dyn #child_dyn + '_> { ::std::boxed::Box::new(l) })
            },
            None => quote! { ::std::boxed::Box::new(<T as #trait_ident>::#ident(self)) },
        };
        trait_items.push(quote! {
            #[doc = #assoc_doc]
            type #assoc<'a>: #child + 'a where Self: 'a;
            #(#docs)*
            fn #ident(&self #index_def) -> #output;
        });
        dyn_items.push(quote! {
            #(#docs)*
            fn #ident(&self #index_def) -> #dyn_output;
        });
        blanket_items.push(quote! {
            fn #ident(&self #index_def) -> #dyn_output {
                #boxed
            }
        });
        deref_items.push(quote! {
            type #assoc<'a> = ::std::boxed::Box<dyn #child_dyn + 'a> where Self: 'a;
            fn #ident(&self #index_def) -> #output {
                #dyn_ident::#ident(&**self #index_name)
            }
        });
        ref_items.push(quote! {
            type #assoc<'a> = <T as #trait_ident>::#assoc<'a> where Self: 'a;
            fn #ident(&self #index_def) -> #output {
                <T as #trait_ident>::#ident(*self #index_name)
            }
        });
    }
    for f in &local.fns {
        let ident = &f.ident;
        let output = &f.output;
        let defs = f.arg_defs();
        let names = f.arg_names();
        let docs = f.docs.iter().map(|d| quote! { #[doc = #d] }).collect::<Vec<_>>();
        trait_items.push(quote! {
            #(#docs)*
            fn #ident(&self, cx: &wire_weaver::CallContext, #(#defs),*) -> impl ::core::future::Future<Output = #output> #future_send;
        });
        // the boxed future borrows both `self` and `cx`
        dyn_items.push(quote! {
            #(#docs)*
            fn #ident<'a>(&'a self, cx: &'a wire_weaver::CallContext, #(#defs),*) -> #box_future<'a, #output>;
        });
        blanket_items.push(quote! {
            fn #ident<'a>(&'a self, cx: &'a wire_weaver::CallContext, #(#defs),*) -> #box_future<'a, #output> {
                ::std::boxed::Box::pin(<T as #trait_ident>::#ident(self, cx, #(#names),*))
            }
        });
        deref_items.push(quote! {
            fn #ident(&self, cx: &wire_weaver::CallContext, #(#defs),*) -> impl ::core::future::Future<Output = #output> #future_send {
                // a block borrowing both: the boxed future's one lifetime can't be named here
                async move { #dyn_ident::#ident(&**self, cx, #(#names),*).await }
            }
        });
        ref_items.push(quote! {
            fn #ident(&self, cx: &wire_weaver::CallContext, #(#defs),*) -> impl ::core::future::Future<Output = #output> #future_send {
                <T as #trait_ident>::#ident(*self, cx, #(#names),*)
            }
        });
    }
    let trait_docs = trait_docs.iter().map(|d| quote! { #[doc = #d] });
    let dyn_doc = format!(
        " [{trait_name}] with boxed futures and boxed levels, for trait objects: every implementor of [{trait_name}] implements it."
    );
    Ok(quote! {
        #(#trait_docs)*
        pub trait #trait_ident #supertraits {
            #(#trait_items)*
        }

        #[doc = #dyn_doc]
        pub trait #dyn_ident #supertraits {
            #(#dyn_items)*
        }

        impl<T: #trait_ident> #dyn_ident for T {
            #(#blanket_items)*
        }

        impl<'d> #trait_ident for ::std::boxed::Box<dyn #dyn_ident + 'd> {
            #(#deref_items)*
        }

        impl<'d> #trait_ident for #shared<dyn #dyn_ident + 'd> {
            #(#deref_items)*
        }

        impl<T: #trait_ident + ?Sized> #trait_ident for &T {
            #(#ref_items)*
        }
    })
}

/// The local traits of the API's levels (made by [gen_local_trait] from the same API, with the same `send`, the root's
/// at `trait_path`) implemented on the generated std client: `client_struct_path` for the root, the client's level
/// structs for the nested levels, each function calling the client's own generated one.
pub fn gen_local_trait_client_impl(
    api_bundle: &ww_self::ApiBundleOwned,
    crate_paths: &CratePaths,
    trait_path: &Path,
    client_struct_path: &Path,
    send: bool,
) -> TokenStream {
    let api_bundle = &CodegenBundle::new(api_bundle, crate_paths);
    let paths = TraitPaths::Ext { root: trait_path };
    let levels = match walk_levels(api_bundle, send) {
        Ok(levels) => levels,
        Err(e) => return quote! { compile_error!(#e); },
    };
    let mut ts = TokenStream::new();
    for (path, local) in &levels {
        match client_impl(api_bundle, &paths, path, local, client_struct_path) {
            Ok(t) => ts.extend(t),
            Err(e) => ts.extend(quote! { compile_error!(#e); }),
        }
    }
    ts
}

/// The client's level struct of the level at `path` (item indices from the root), `'a` its lifetime; the root's is
/// `client_struct_path`.
fn client_struct(
    api_bundle: &CodegenBundle,
    path: &[usize],
    client_struct_path: &Path,
    lifetime: TokenStream,
) -> TokenStream {
    if path.is_empty() {
        return quote! { #client_struct_path };
    }
    let mut level = &api_bundle.root;
    let mut modules = vec![super::util::mod_name(level, api_bundle)];
    for &item in path {
        level = level.items[item]
            .get_as_level(api_bundle)
            .expect("a level the walk reached");
        modules.push(super::util::mod_name(level, api_bundle));
    }
    let name = Ident::new(
        &format!("{}_client", modules.last().expect("a level")).to_case(Case::Pascal),
        Span::call_site(),
    );
    quote! { #(#modules)::* :: #name<#lifetime> }
}

fn client_impl(
    api_bundle: &CodegenBundle,
    paths: &TraitPaths<'_>,
    path: &[usize],
    local: &LocalLevel<'_>,
    client_struct_path: &Path,
) -> Result<TokenStream, String> {
    let trait_path = paths.path(api_bundle, local.level, false)?;
    let (generics, self_ty) = if path.is_empty() {
        (quote! {}, client_struct(api_bundle, path, client_struct_path, quote! {}))
    } else {
        (
            quote! { <'i> },
            client_struct(api_bundle, path, client_struct_path, quote! { 'i }),
        )
    };
    let accessors = local.accessors.iter().map(|a| {
        let mut child_path = path.to_vec();
        child_path.push(a.item);
        let child = client_struct(api_bundle, &child_path, client_struct_path, quote! { 'a });
        let (ident, assoc) = (&a.ident, &a.assoc);
        match &a.index {
            Some(ty) => quote! {
                type #assoc<'a> = #child where Self: 'a;
                fn #ident(&self, index: #ty) -> Result<Self::#assoc<'_>, wire_weaver::call::Error> {
                    Ok(Self::#ident(self, index))
                }
            },
            None => quote! {
                type #assoc<'a> = #child where Self: 'a;
                fn #ident(&self) -> Self::#assoc<'_> {
                    Self::#ident(self)
                }
            },
        }
    });
    let fns = local.fns.iter().map(|f| {
        let ident = &f.ident;
        let defs = f.arg_defs();
        let output = &f.output;
        let body = &f.client_body;
        quote! {
            async fn #ident(&self, cx: &wire_weaver::CallContext, #(#defs),*) -> #output {
                #body
            }
        }
    });
    Ok(quote! {
        impl #generics #trait_path for #self_ty {
            #(#accessors)*
            #(#fns)*
        }
    })
}

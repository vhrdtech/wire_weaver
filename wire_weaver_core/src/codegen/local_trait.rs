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
//! A `keyed!` array has `<array>_index_of`, `<array>_key` and `<array>_keys`, and an array of levels a provided
//! `<array>_by` that finds the element by its key.
//!
//! Every level's trait is implemented for `wire_weaver::authorize::Authorized<T, A>` (SRV-72): a function asks the
//! hook with its access and the steps to its item (ids, indices, effects and metadata from the model, as constants),
//! then forwards; an accessor wraps the level it hands out.
//!
//! Left out, and listed in each trait's docs: observing, sinks, counted, reliable or batched streams, mount slots.
use crate::codegen::CodegenBundle;
use crate::codegen::ty_def::{TyPos, ty_def, ty_def_by_idx};
use crate::transform::CratePaths;
use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use std::collections::BTreeSet;
use syn::Path;
use ww_self::{
    ApiItemKindOwned, ApiItemOwned, ApiLevelOwned, Batching, ClaimMode, Delivery, Effects,
    MetaOwned, Multiplicity, PropertyAccess, SharesOwned, TrackLayout, TrackOwned, TypeOwned,
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
    /// The index of a key of a `keyed!` array, `<array>_index_of`.
    IndexOf,
    /// The key of an element of a `keyed!` array, `<array>_key`.
    Key,
    /// The `keys` stream of a `keyed!` array, `<array>_keys`.
    Keys,
}

/// The provided `<array>_by` of a `keyed!` array of levels: its `index_of`, then its accessor.
pub(crate) struct KeyedBy {
    ident: Ident,
    index_of: Ident,
    accessor: Ident,
    assoc: Ident,
    key: TokenStream,
}

/// A `keyed!` array's index and event types in a local trait: the index the array's accessor takes (`u32`, or the
/// array's own index type), the event `KeyEvent<index, key>`, and whether the wire's index differs (`UNib32` of an
/// array declared `name[]`).
pub(crate) struct KeyedTypes {
    pub(crate) index: TokenStream,
    pub(crate) key: TokenStream,
    /// `ww_keyed::KeyEvent`, the path without its arguments.
    pub(crate) event_path: TokenStream,
    /// The array is declared `name[]`: the wire's index is `UNib32`, the trait's `u32`.
    pub(crate) unib_index: bool,
}

impl KeyedTypes {
    /// The types of the `keyed!` companion `keys` of the array `array`.
    pub(crate) fn new(
        api_bundle: &CodegenBundle,
        array: &ApiItemOwned,
        keys: &ApiItemOwned,
    ) -> Result<Self, String> {
        let ApiItemKindOwned::Keys { key, event_ty, .. } = &keys.kind else {
            return Err(format!("`{}` is not a keyed! companion", keys.ident));
        };
        let index = index_ty(api_bundle, &array.multiplicity)
            .ok_or_else(|| format!("keyed! `{}`: `{}` is not an array", keys.ident, array.ident))?;
        let key = ty_def(api_bundle, key, true, TyPos::Arg).map_err(|e| e.to_string())?;
        let event = ty_def(api_bundle, event_ty, true, TyPos::Arg).map_err(|e| e.to_string())?;
        let mut event: syn::TypePath = syn::parse2(event)
            .map_err(|e| format!("keyed! `{}`: the event type: {e}", keys.ident))?;
        if let Some(last) = event.path.segments.last_mut() {
            last.arguments = syn::PathArguments::None;
        }
        Ok(KeyedTypes {
            index,
            key,
            event_path: quote! { #event },
            unib_index: matches!(
                array.multiplicity,
                Multiplicity::Array {
                    index_type_idx: None
                }
            ),
        })
    }

    /// The event type as the trait's `keys` stream gives it.
    pub(crate) fn event(&self) -> TokenStream {
        let KeyedTypes {
            index,
            key,
            event_path,
            ..
        } = self;
        quote! { #event_path<#index, #key> }
    }
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
    /// The provided `<array>_by` of its `keyed!` arrays of levels.
    keyed_by: Vec<KeyedBy>,
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
        keyed_by: vec![],
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
        // the keys of `mounts!` go with its slots, which a local trait leaves out
        ApiItemKindOwned::Keys { of, .. }
            if out.level.items.iter().any(|array| {
                array.id == *of && matches!(array.kind, ApiItemKindOwned::Mount { .. })
            }) =>
        {
            out.skipped
                .push(format!("`{}` (keys of mount slots)", item.ident));
        }
        ApiItemKindOwned::Keys { of, .. } => {
            keyed_fns(api_bundle, out, item_index, item, *of, send)?;
        }
        // TODO(SRV-76): a local trait has no view of a mounted API
        ApiItemKindOwned::Mount { .. } => {
            out.skipped.push(format!("`{}` (mount slot)", item.ident));
        }
    }
    Ok(())
}

/// The functions of the `keyed!` companion `keys` of the array `of` of `out`'s level: `<array>_index_of`,
/// `<array>_key` and `<array>_keys`, and the provided `<array>_by` of an array of levels.
fn keyed_fns(
    api_bundle: &CodegenBundle,
    out: &mut LocalLevel<'_>,
    item_index: usize,
    keys: &ApiItemOwned,
    of: shrink_wrap::UNib32,
    send: bool,
) -> Result<(), String> {
    let array = out
        .level
        .items
        .iter()
        .find(|item| item.id == of)
        .ok_or_else(|| format!("keyed! `{}`: no item {} in the level", keys.ident, of.0))?;
    let types = KeyedTypes::new(api_bundle, array, keys)?;
    let KeyedTypes { index, key, .. } = &types;
    let name = |op: &str| Ident::new(&format!("{}_{op}", array.ident), Span::call_site());
    let (index_of, key_fn, keys_fn) = (name("index_of"), name("key"), name("keys"));
    let event = types.event();
    let handle = if send {
        quote! { wire_weaver::call::ItemStream<#event> }
    } else {
        quote! { wire_weaver::call::LocalItemStream<#event> }
    };
    // the client works with `u32` indices; an array's own index type comes from it as the server takes it
    let from_u32 = if types.unib_index {
        quote! {}
    } else {
        quote! {
            .and_then(|index| index.map(|index| <#index as ::core::convert::TryFrom<u32>>::try_from(index).map_err(|_| wire_weaver::call::Error::from_kind(wire_weaver::call::ErrorKind::BadIndex))).transpose())
        }
    };
    let array_ident = &array.ident;
    out.fns.push(LocalFn {
        client_body: quote! { Self::#index_of(self, &key).await.map_err(wire_weaver::call::Error::from) #from_u32 },
        ident: index_of.clone(),
        docs: vec![format!(" The index of the element of `{array_ident}[]` with `key`, `None` when there is none.")],
        index: None,
        args: vec![(Ident::new("key", Span::call_site()), key.clone())],
        output: quote! { Result<Option<#index>, wire_weaver::call::Error> },
        kind: LocalFnKind::IndexOf,
        item: item_index,
    });
    out.fns.push(LocalFn {
        client_body: quote! { Self::#key_fn(self, index).await.map_err(::core::convert::Into::into) },
        ident: key_fn,
        docs: vec![format!(" The key of the element of `{array_ident}[]` at `index`.")],
        index: Some(index.clone()),
        args: vec![],
        output: quote! { Result<#key, wire_weaver::call::Error> },
        kind: LocalFnKind::Key,
        item: item_index,
    });
    out.fns.push(LocalFn {
        client_body: quote! { Self::#keys_fn(self).await.map(|s| { let s: #handle = s.into_item_stream::<#index>(); s }).map_err(::core::convert::Into::into) },
        ident: keys_fn,
        docs: vec![format!(" Elements of `{array_ident}[]` as they appear and go away.")],
        index: None,
        args: vec![],
        output: quote! { Result<#handle, wire_weaver::call::Error> },
        kind: LocalFnKind::Keys,
        item: item_index,
    });
    if let ApiItemKindOwned::Trait { .. } = array.kind {
        out.keyed_by.push(KeyedBy {
            ident: name("by"),
            index_of,
            accessor: Ident::new(array_ident, Span::call_site()),
            assoc: Ident::new(&array_ident.to_case(Case::Pascal), Span::call_site()),
            key: key.clone(),
        });
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
        let docs = a
            .docs
            .iter()
            .map(|d| quote! { #[doc = #d] })
            .collect::<Vec<_>>();
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
        let docs = f
            .docs
            .iter()
            .map(|d| quote! { #[doc = #d] })
            .collect::<Vec<_>>();
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
    for by in &local.keyed_by {
        let KeyedBy {
            ident,
            index_of,
            accessor,
            assoc,
            key,
        } = by;
        let doc = format!(
            " The element of `{accessor}[]` with `key`, `None` when there is none: `{index_of}`, then `{accessor}`."
        );
        trait_items.push(quote! {
            #[doc = #doc]
            fn #ident(&self, cx: &wire_weaver::CallContext, key: #key) -> impl ::core::future::Future<Output = Result<Option<Self::#assoc<'_>>, wire_weaver::call::Error>> #future_send {
                async move {
                    match self.#index_of(cx, key).await? {
                        Some(index) => self.#accessor(index).map(Some),
                        None => Ok(None),
                    }
                }
            }
        });
    }
    trait_docs.push(String::new());
    trait_docs.push(
        " `wire_weaver::authorize::Authorized<T, A>` implements it for any implementor `T`, asking the hook `A` before every call (SRV-72).".to_owned(),
    );
    let authorized = authorized_impl(api_bundle, paths, local, send)?;
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

        #authorized
    })
}

/// The step of `item` in an authorization check (SRV-72), `index` its element's index as a `u32` expression: its
/// effects and metadata as the API's model has them, the metadata in a `static` built on first use.
fn step(item: &ApiItemOwned, index: Option<TokenStream>) -> TokenStream {
    let ident = &item.ident;
    let id = item.id.0;
    step_tokens(ident, id, index, item.effects(), item.meta.as_ref())
}

fn step_tokens(
    ident: &str,
    id: u32,
    index: Option<TokenStream>,
    effects: Effects,
    meta: Option<&MetaOwned>,
) -> TokenStream {
    let index = match index {
        Some(index) => quote! { Some(#index) },
        None => quote! { None },
    };
    let effects = effects_tokens(effects);
    let meta = match meta {
        Some(meta) => {
            let meta = meta_tokens(meta);
            quote! {
                {
                    static META: ::std::sync::LazyLock<wire_weaver::ww_self::MetaOwned> = ::std::sync::LazyLock::new(|| #meta);
                    Some(&*META)
                }
            }
        }
        None => quote! { None },
    };
    quote! { wire_weaver::authorize::Step::new(#ident, #id, #index, #effects, #meta) }
}

fn effects_tokens(effects: Effects) -> TokenStream {
    let bits = effects.bits();
    quote! { wire_weaver::ww_self::Effects(wire_weaver::shrink_wrap::UNib32(#bits)) }
}

/// `meta` as an expression of `ww_self::MetaOwned`.
fn meta_tokens(meta: &MetaOwned) -> TokenStream {
    let MetaOwned {
        claim,
        shares,
        labels,
        effects,
        track,
    } = meta;
    let claim = match claim {
        Some(ClaimMode::Exclusive) => quote! { Some(wire_weaver::ww_self::ClaimMode::Exclusive) },
        Some(ClaimMode::Shared) => quote! { Some(wire_weaver::ww_self::ClaimMode::Shared) },
        Some(ClaimMode::Observe) => quote! { Some(wire_weaver::ww_self::ClaimMode::Observe) },
        None => quote! { None },
    };
    let strings = |list: &[String]| quote! { ::std::vec![#(::std::string::String::from(#list)),*] };
    let shares = shares.iter().map(|s| {
        let SharesOwned {
            index,
            pins,
            peripherals,
        } = s;
        let index = match index {
            Some(i) => {
                let i = i.0;
                quote! { Some(wire_weaver::shrink_wrap::UNib32(#i)) }
            }
            None => quote! { None },
        };
        let (pins, peripherals) = (strings(pins), strings(peripherals));
        quote! { wire_weaver::ww_self::SharesOwned { index: #index, pins: #pins, peripherals: #peripherals } }
    });
    let labels = strings(labels);
    let effects = effects_tokens(*effects);
    let track = match track {
        Some(TrackOwned {
            layout,
            dims,
            flags,
        }) => {
            let layout = match layout {
                TrackLayout::Points => quote! { Points },
                TrackLayout::Spans => quote! { Spans },
                TrackLayout::Series => quote! { Series },
            };
            let dims = strings(dims);
            quote! {
                Some(wire_weaver::ww_self::TrackOwned {
                    layout: wire_weaver::ww_self::TrackLayout::#layout,
                    dims: #dims,
                    flags: #flags,
                })
            }
        }
        None => quote! { None },
    };
    quote! {
        wire_weaver::ww_self::MetaOwned {
            claim: #claim,
            shares: ::std::vec![#(#shares),*],
            labels: #labels,
            effects: #effects,
            track: #track,
        }
    }
}

/// `index` (the trait's index argument, `u32` or the array's own type) as the `u32` of a path.
fn index_u32() -> TokenStream {
    quote! { ::core::convert::Into::<u32>::into(::core::clone::Clone::clone(&index)) }
}

/// The level's trait implemented for `wire_weaver::authorize::Authorized<T, A>` (SRV-72): each function asks the
/// hook with the access and the steps to its item, then forwards to `T`; each accessor wraps the level `T` hands out
/// with the same hook and the step to it.
fn authorized_impl(
    api_bundle: &CodegenBundle,
    paths: &TraitPaths<'_>,
    local: &LocalLevel<'_>,
    send: bool,
) -> Result<TokenStream, String> {
    let trait_path = paths.path(api_bundle, local.level, false)?;
    // a level of another crate may have its trait in the `Send` form, which a wrapper holding `&A` meets only with
    // a `Sync` hook
    let foreign_child = local
        .accessors
        .iter()
        .any(|a| a.level.crate_idx != api_bundle.root.crate_idx);
    let hook_bounds = match (send, foreign_child) {
        (true, _) => quote! { wire_weaver::authorize::Authorize + Send + Sync },
        (false, true) => quote! { wire_weaver::authorize::Authorize + Sync },
        (false, false) => quote! { wire_weaver::authorize::Authorize },
    };
    let mut items = vec![];
    for a in &local.accessors {
        let (ident, assoc) = (&a.ident, &a.assoc);
        let item = &local.level.items[a.item];
        items.push(match &a.index {
            Some(ty) => {
                let step = step(item, Some(index_u32()));
                quote! {
                    type #assoc<'a> = wire_weaver::authorize::Authorized<<T as #trait_path>::#assoc<'a>, &'a A> where Self: 'a;
                    fn #ident(&self, index: #ty) -> Result<Self::#assoc<'_>, wire_weaver::call::Error> {
                        let step = #step;
                        <T as #trait_path>::#ident(self.inner(), index).map(|level| self.__level(level, step))
                    }
                }
            }
            None => {
                let step = step(item, None);
                quote! {
                    type #assoc<'a> = wire_weaver::authorize::Authorized<<T as #trait_path>::#assoc<'a>, &'a A> where Self: 'a;
                    fn #ident(&self) -> Self::#assoc<'_> {
                        self.__level(<T as #trait_path>::#ident(self.inner()), #step)
                    }
                }
            }
        });
    }
    for f in &local.fns {
        let item = &local.level.items[f.item];
        let index = f.index.as_ref().map(|_| index_u32());
        let (access, steps) = match f.kind {
            LocalFnKind::Method => (quote! { Call }, vec![step(item, index)]),
            LocalFnKind::Read => (quote! { Read }, vec![step(item, index)]),
            LocalFnKind::Write => (quote! { Write }, vec![step(item, index)]),
            LocalFnKind::Stream => (quote! { Stream }, vec![step(item, index)]),
            LocalFnKind::ValidIndices => (quote! { Read }, vec![step(item, None)]),
            LocalFnKind::IndexOf | LocalFnKind::Key | LocalFnKind::Keys => {
                let (access, op) = match f.kind {
                    LocalFnKind::IndexOf => (quote! { Call }, ww_self::keys::INDEX_OF),
                    LocalFnKind::Key => (quote! { Read }, ww_self::keys::KEY),
                    _ => (quote! { Stream }, ww_self::keys::KEYS),
                };
                let name = ww_self::keys::item_name(op).expect("a companion's own id");
                let companion = step_tokens(name, op, index, Effects::READ, None);
                (access, vec![step(item, None), companion])
            }
        };
        let ident = &f.ident;
        let output = &f.output;
        let defs = f.arg_defs();
        let names = f.arg_names();
        items.push(quote! {
            async fn #ident(&self, cx: &wire_weaver::CallContext, #(#defs),*) -> #output {
                self.__check(cx, wire_weaver::authorize::Access::#access, &[#(#steps),*])
                    .resolve()
                    .await
                    .map_err(wire_weaver::call::Error::from)?;
                <T as #trait_path>::#ident(self.inner(), cx, #(#names),*).await
            }
        });
    }
    Ok(quote! {
        impl<T: #trait_path, A: #hook_bounds> #trait_path for wire_weaver::authorize::Authorized<T, A> {
            #(#items)*
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
        (
            quote! {},
            client_struct(api_bundle, path, client_struct_path, quote! {}),
        )
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

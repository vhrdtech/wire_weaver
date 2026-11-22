//! A server forwarding to the API's local traits (SRV-49, CLT-58): `ww_codegen!(api :: Api for Server, server = true,
//! serve_trait = "api::Name")` generates the handler of every item the local traits (CLT-57) express, at every
//! level, each reaching its level through the accessors from the service `wire_weaver_server::Serves` gives and
//! calling the trait there, and the backend traits a host runtime calls.
//!
//! The handlers are the ones the dispatcher calls anyway (one dispatcher): calls and reads take `&self` and run side
//! by side, writes and stream opens take `&mut self` and run alone. A resource array's indices are checked through
//! its `<array>_valid_indices`, an index the accessor refuses is answered `BadIndex`. A stream open registers the
//! trait's item stream as a pump of the connection, named by the stream and the element's indices, whose end closes
//! the client's stream; the client's close removes it. Items the traits leave out are handlers written by hand
//! beside the generated ones.
//!
//! `level_forwards` is the per-level part: the path from the service to a level and the call of each of its
//! functions, for any wrapper that forwards to a local implementation.
use crate::codegen::CodegenBundle;
use crate::codegen::local_trait::{
    KeyedTypes, LocalFn, LocalFnKind, LocalLevel, TraitPaths, walk_levels,
};
use crate::codegen::ty_def::{TyPos, ty_def};
use crate::transform::CratePaths;
use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::quote;
use syn::Path;
use ww_self::{ApiItemKindOwned, ApiItemOwned, ApiLevelOwned, Multiplicity, TypeOwned};

/// What [gen_serve_trait] needs besides the API.
pub struct ServeTraitConfig<'a> {
    /// The root's local trait, `api::Name`.
    pub trait_path: &'a Path,
    /// The server struct the handlers are implemented on.
    pub server: &'a Path,
    /// The medium of `Context`, `()` if None.
    pub medium: Option<&'a Path>,
    /// The API's `*_FULL_GID` const, the server's version.
    pub full_gid: TokenStream,
}

/// One step from the service to a level: the accessor of a nested level, with the element's index for an array.
pub(crate) struct Step {
    /// The local trait of the level the accessor is on.
    trait_path: TokenStream,
    accessor: Ident,
    /// Position of the element's index in the handler's `index_chain`, and the array's own index type.
    index: Option<(usize, Option<TokenStream>)>,
}

/// One level as a forwarding server or wrapper reaches it.
pub(crate) struct LevelForwards<'b> {
    pub(crate) local: LocalLevel<'b>,
    /// The level's local trait.
    pub(crate) trait_path: TokenStream,
    /// From the service to the level.
    pub(crate) steps: Vec<Step>,
    /// The item idents from the root, the handlers' prefix (`periph_channel`), empty at the root.
    pub(crate) prefix: Vec<String>,
    /// Indices in the chain of a handler of an item of this level, the item's own not counted.
    pub(crate) depth: usize,
}

/// Every level of the API, with how to reach it and how to call it ([LevelForwards]). `root` is the root's local
/// trait.
pub(crate) fn level_forwards<'b>(
    api_bundle: &'b CodegenBundle,
    root: &Path,
) -> Result<Vec<LevelForwards<'b>>, String> {
    let paths = TraitPaths::Ext { root };
    // the handlers don't name the stream type, so both forms of the trait are served by the same code
    let levels = walk_levels(api_bundle, true)?;
    let mut out: Vec<LevelForwards<'b>> = Vec::with_capacity(levels.len());
    for (path, local) in levels {
        let trait_path = paths.path(api_bundle, local.level, false)?;
        let mut steps = vec![];
        let mut prefix = vec![];
        let mut depth = 0;
        let mut level: &ApiLevelOwned = &api_bundle.root;
        for &item_index in &path {
            let item = &level.items[item_index];
            let index = match &item.multiplicity {
                Multiplicity::Flat => None,
                Multiplicity::Array { index_type_idx } => {
                    let ty = index_type_idx.map(|idx| {
                        super::super::ty_def::ty_def_by_idx(api_bundle, idx.0, true, TyPos::Arg)
                            .expect("the index type")
                    });
                    depth += 1;
                    Some((depth - 1, ty))
                }
            };
            steps.push(Step {
                trait_path: paths.path(api_bundle, level, false)?,
                accessor: Ident::new(&item.ident, Span::call_site()),
                index,
            });
            prefix.push(item.ident.clone());
            level = item
                .get_as_level(api_bundle)
                .map_err(|e| format!("`{}`: {e}", item.ident))?;
        }
        out.push(LevelForwards {
            local,
            trait_path,
            steps,
            prefix,
            depth,
        });
    }
    Ok(out)
}

/// The index at `position` of the handler's `index_chain` as the trait takes it: a `u32`, or the array's own index
/// type through `TryFrom<u32>`, `BadIndex` when it doesn't convert. `fail` turns a `wire_weaver::call::Error` named
/// `e` into the handler's answer.
fn index_expr(position: usize, ty: Option<&TokenStream>, fail: &TokenStream) -> TokenStream {
    let raw = quote! { index_chain[#position].get().0 };
    match ty {
        None => raw,
        Some(ty) => quote! {
            match <#ty as ::core::convert::TryFrom<u32>>::try_from(#raw) {
                Ok(index) => index,
                Err(_) => {
                    let e = wire_weaver::call::Error::from_kind(wire_weaver::call::ErrorKind::BadIndex);
                    return #fail;
                }
            }
        },
    }
}

impl LevelForwards<'_> {
    /// Statements binding `ww_level` to this level, from `service` (a reference to the service); an accessor's error
    /// returns `fail`, which reads the `wire_weaver::call::Error` named `e`.
    pub(crate) fn reach(&self, service: TokenStream, fail: &TokenStream) -> TokenStream {
        let mut ts = quote! { let ww_level = #service; };
        for step in &self.steps {
            let Step {
                trait_path,
                accessor,
                index,
            } = step;
            ts.extend(match index {
                None => quote! { let ww_level = &#trait_path::#accessor(ww_level); },
                Some((position, ty)) => {
                    let index = index_expr(*position, ty.as_ref(), fail);
                    quote! {
                        let ww_level = &match #trait_path::#accessor(ww_level, #index) {
                            Ok(ww_level) => ww_level,
                            Err(e) => return #fail,
                        };
                    }
                }
            });
        }
        ts
    }

    /// The call of `f` on `ww_level` with `cx`, awaited; the index of an item of an array is the chain's last.
    pub(crate) fn call(&self, f: &LocalFn, cx: TokenStream, fail: &TokenStream) -> TokenStream {
        let trait_path = &self.trait_path;
        let ident = &f.ident;
        let index = f.index.as_ref().map(|ty| {
            let item = self.indexed_item(f);
            let own = match &item.multiplicity {
                Multiplicity::Array {
                    index_type_idx: Some(_),
                } => Some(ty),
                _ => None,
            };
            index_expr(self.depth, own, fail)
        });
        let index = index.into_iter();
        let args = f.args.iter().map(|(n, _)| n);
        quote! { #trait_path::#ident(ww_level, #cx, #(#index,)* #(#args),*).await }
    }

    /// The item whose index `f` takes: the item itself, or the array a `keyed!` companion keys.
    fn indexed_item(&self, f: &LocalFn) -> &ApiItemOwned {
        let item = &self.local.level.items[f.item];
        match &item.kind {
            ApiItemKindOwned::Keys { of, .. } => self
                .local
                .level
                .items
                .iter()
                .find(|array| array.id == *of)
                .unwrap_or(item),
            _ => item,
        }
    }

    /// The stream serializer of this level: `stream_data_ser()` and its accessors down to the level, each array's
    /// element from the handler's `index_chain`.
    fn serializer(&self) -> TokenStream {
        let steps = self.steps.iter().map(|step| {
            let accessor = &step.accessor;
            match step.index {
                None => quote! { .#accessor() },
                Some((position, _)) => quote! { .#accessor(index_chain[#position].get().0) },
            }
        });
        quote! { stream_data_ser() #(#steps)* }
    }

    /// The `index_chain` argument of the handler of an item of this level, `own`: the item is an array's.
    fn chain_def(&self, own: bool) -> TokenStream {
        let len = self.depth + usize::from(own);
        if len == 0 {
            quote! {}
        } else {
            quote! { index_chain: [ww_client_server::ResourceIndex; #len], }
        }
    }

    /// `name` with the level's prefix, as the dispatcher names handlers: `periph_channel_run`.
    fn prefixed(&self, name: &str) -> String {
        self.prefix
            .iter()
            .map(String::as_str)
            .chain([name])
            .collect::<Vec<_>>()
            .join("_")
    }
}

/// The forwarding handlers of `config.server` and its `WireWeaverAsyncApiBackend` and
/// `WireWeaverSharedApiBackend` implementations.
pub fn gen_serve_trait(
    api_bundle: &ww_self::ApiBundleOwned,
    crate_paths: &CratePaths,
    config: &ServeTraitConfig<'_>,
) -> TokenStream {
    let api_bundle = &CodegenBundle::new(api_bundle, crate_paths);
    let levels = match level_forwards(api_bundle, config.trait_path) {
        Ok(levels) => levels,
        Err(e) => return quote! { compile_error!(#e); },
    };
    let medium = match config.medium {
        Some(m) => quote! { #m },
        None => quote! { () },
    };
    let cx_ty = quote! { &mut wire_weaver::Context<'_, impl wire_weaver::EventOut, #medium> };
    let mut handlers = TokenStream::new();
    for level in &levels {
        for f in &level.local.fns {
            handlers.extend(handler(api_bundle, level, f, &cx_ty));
        }
    }

    let server = config.server;
    let full_gid = &config.full_gid;
    quote! {
        impl #server {
            #handlers
        }

        /// What a host runtime calls (SRV-12), generated with `serve_trait` (SRV-49).
        impl wire_weaver::WireWeaverAsyncApiBackend for #server {
            type Medium = #medium;

            async fn process_bytes<'a>(
                &mut self,
                out: &mut wire_weaver::EventWriter<'_, impl wire_weaver::MessageSink>,
                medium: #medium,
                data: &[u8],
                scratch: &'a mut [u8],
            ) -> Result<&'a [u8], wire_weaver::shrink_wrap::Error> {
                self.process_request_bytes(data, scratch, out, medium).await
            }

            fn version(&self) -> wire_weaver::ww_version::FullVersion<'_> {
                #full_gid
            }
        }

        impl wire_weaver::WireWeaverSharedApiBackend for #server {
            async fn process_exclusive_bytes<'a>(
                &mut self,
                origin: &wire_weaver::CallContext,
                out: &mut wire_weaver::EventWriter<'_, impl wire_weaver::MessageSink>,
                medium: #medium,
                data: &[u8],
                scratch: &'a mut [u8],
            ) -> Result<&'a [u8], wire_weaver::shrink_wrap::Error> {
                self.process_request_from(origin, data, scratch, out, medium).await
            }

            fn is_shared(data: &[u8]) -> bool {
                #server::is_shared(data)
            }

            async fn process_shared_bytes<'a>(
                &self,
                origin: &wire_weaver::CallContext,
                out: &mut wire_weaver::EventWriter<'_, impl wire_weaver::MessageSink>,
                medium: #medium,
                data: &[u8],
                scratch: &'a mut [u8],
            ) -> Result<&'a [u8], wire_weaver::shrink_wrap::Error> {
                self.process_shared_from(origin, data, scratch, out, medium).await
            }
        }
    }
}

/// The handler of one function of a level's trait.
fn handler(
    api_bundle: &CodegenBundle,
    level: &LevelForwards<'_>,
    f: &LocalFn,
    cx_ty: &TokenStream,
) -> TokenStream {
    let item = &level.local.level.items[f.item];
    let service = quote! { wire_weaver_server::Serves::service(self) };
    let arg_defs = f
        .args
        .iter()
        .map(|(n, t)| quote! { #n: #t })
        .collect::<Vec<_>>();
    let failed = |result: TokenStream| quote! { #result::Failed(wire_weaver::Failure::from(e)) };
    match f.kind {
        LocalFnKind::Method => {
            let ApiItemKindOwned::Method { return_ty, .. } = &item.kind else {
                unreachable!("a method's function comes from a method")
            };
            let (output, forward) = handler_output(api_bundle, return_ty.as_ref());
            let forward = if forward {
                quote! { call_result }
            } else {
                quote! { call }
            };
            let fail = failed(quote! { wire_weaver::RpcResult });
            let reach = level.reach(service, &fail);
            let call = level.call(f, quote! { cx.call() }, &fail);
            let ident = Ident::new(&level.prefixed(&item.ident), Span::call_site());
            let chain = level.chain_def(f.index.is_some());
            quote! {
                async fn #ident(&self, cx: #cx_ty, #chain #(#arg_defs),*) -> wire_weaver::RpcResult<#output> {
                    #reach
                    wire_weaver::forward::#forward(#call)
                }
            }
        }
        LocalFnKind::Read | LocalFnKind::Write => {
            let ApiItemKindOwned::Property {
                ty, write_err_ty, ..
            } = &item.kind
            else {
                unreachable!("a property's function comes from a property")
            };
            let value_ty = ty_def(api_bundle, ty, true, TyPos::Arg).expect("the trait has it");
            let err_ty = match write_err_ty {
                Some(e) => {
                    let e = ty_def(api_bundle, e, true, TyPos::Arg).expect("the trait has it");
                    quote! { wire_weaver::Error<'_, #e> }
                }
                None => quote! { wire_weaver::CommonError<'_> },
            };
            let name = level.prefixed(&item.ident);
            let chain = level.chain_def(f.index.is_some());
            if f.kind == LocalFnKind::Read {
                let fail = failed(quote! { wire_weaver::GetResult });
                let reach = level.reach(service, &fail);
                let call = level.call(f, quote! { cx.call() }, &fail);
                let getter = Ident::new(&format!("get_{name}"), Span::call_site());
                quote! {
                    async fn #getter(&self, cx: #cx_ty, #chain) -> wire_weaver::GetResult<#value_ty, #err_ty> {
                        #reach
                        wire_weaver::forward::read(#call)
                    }
                }
            } else {
                let fail = failed(quote! { wire_weaver::SetResult });
                let reach = level.reach(service, &fail);
                let call = level.call(f, quote! { cx.call() }, &fail);
                let setter = Ident::new(&format!("set_{name}"), Span::call_site());
                let forward = if write_err_ty.is_some() {
                    quote! { write_result }
                } else {
                    quote! { write }
                };
                quote! {
                    async fn #setter(&mut self, cx: #cx_ty, #chain #(#arg_defs),*) -> wire_weaver::SetResult<#err_ty> {
                        #reach
                        wire_weaver::forward::#forward(#call)
                    }
                }
            }
        }
        LocalFnKind::Stream => {
            let stream = item.kind.as_stream_fn().expect("a stream");
            stream_handlers(api_bundle, level, f, cx_ty, stream.return_ty, None)
        }
        LocalFnKind::IndexOf | LocalFnKind::Key | LocalFnKind::Keys => {
            keyed_handler(api_bundle, level, f, cx_ty)
        }
        LocalFnKind::ValidIndices => {
            // the dispatcher asks it before every request under the array, and answers the indices read with it
            let fail = quote! { Err(wire_weaver::Failure::from(e)) };
            let reach = level.reach(service, &fail);
            let call = level.call(f, quote! { cx }, &fail);
            let chain_name = ["root".to_string()]
                .into_iter()
                .chain(level.prefix.iter().cloned())
                .chain([item.ident.clone()])
                .collect::<Vec<_>>()
                .join("_");
            let ident = Ident::new(&format!("valid_indices_{chain_name}"), Span::call_site());
            let chain = level.chain_def(false);
            quote! {
                async fn #ident(
                    &self,
                    cx: &wire_weaver::CallContext,
                    #chain
                ) -> Result<wire_weaver::ValidIndicesOwned, wire_weaver::Failure> {
                    #reach
                    #call.map_err(wire_weaver::Failure::from)
                }
            }
        }
    }
}

/// The handler of a function of a `keyed!` companion: `index_of`, the getter of `key[]`, or the open, close and
/// sideband handlers of the `keys` stream (the dispatcher's `keys_stream_handlers`).
fn keyed_handler(
    api_bundle: &CodegenBundle,
    level: &LevelForwards<'_>,
    f: &LocalFn,
    cx_ty: &TokenStream,
) -> TokenStream {
    let keys = &level.local.level.items[f.item];
    let array = level.indexed_item(f);
    let types = match KeyedTypes::new(api_bundle, array, keys) {
        Ok(types) => types,
        Err(e) => return quote! { compile_error!(#e); },
    };
    let service = quote! { wire_weaver_server::Serves::service(self) };
    let name = level.prefixed(&f.ident.to_string());
    let chain = level.chain_def(f.index.is_some());
    let unib = quote! { wire_weaver::shrink_wrap::UNib32 };
    match f.kind {
        LocalFnKind::IndexOf => {
            let fail = quote! { wire_weaver::RpcResult::Failed(wire_weaver::Failure::from(e)) };
            let reach = level.reach(service, &fail);
            let call = level.call(f, quote! { cx.call() }, &fail);
            let (wire_index, to_wire) = if types.unib_index {
                (unib.clone(), quote! { .map(|index| index.map(#unib)) })
            } else {
                (types.index.clone(), quote! {})
            };
            let ident = Ident::new(&name, Span::call_site());
            let key = &types.key;
            quote! {
                async fn #ident(&self, cx: #cx_ty, #chain key: #key) -> wire_weaver::RpcResult<Option<#wire_index>> {
                    #reach
                    wire_weaver::forward::call(#call #to_wire)
                }
            }
        }
        LocalFnKind::Key => {
            let fail = quote! { wire_weaver::GetResult::Failed(wire_weaver::Failure::from(e)) };
            let reach = level.reach(service, &fail);
            let call = level.call(f, quote! { cx.call() }, &fail);
            let getter = Ident::new(&format!("get_{name}"), Span::call_site());
            let key = &types.key;
            quote! {
                async fn #getter(&self, cx: #cx_ty, #chain) -> wire_weaver::GetResult<#key, wire_weaver::CommonError<'_>> {
                    #reach
                    wire_weaver::forward::read(#call)
                }
            }
        }
        _ => {
            let path = &types.event_path;
            let to_wire = types.unib_index.then(|| {
                quote! {
                    |event| match event {
                        #path::Added { index, key } => #path::Added { index: #unib(index), key },
                        #path::Removed { index } => #path::Removed { index: #unib(index) },
                        #path::Unknown(other) => #path::Unknown(other),
                    }
                }
            });
            stream_handlers(api_bundle, level, f, cx_ty, None, to_wire)
        }
    }
}

/// A handler's output for an item returning `return_ty`, and whether it is declared `-> Result<T, E>`.
fn handler_output(
    api_bundle: &CodegenBundle,
    return_ty: Option<&TypeOwned>,
) -> (TokenStream, bool) {
    let Some(return_ty) = return_ty else {
        return (quote! { () }, false);
    };
    match return_ty.get_in_line(api_bundle).expect("the trait has it") {
        TypeOwned::Result { ok_ty, err_ty } => {
            let ok_ty = ty_def(api_bundle, ok_ty, true, TyPos::Arg).expect("the trait has it");
            let err_ty = ty_def(api_bundle, err_ty, true, TyPos::Arg).expect("the trait has it");
            (
                quote! { Result<#ok_ty, wire_weaver::Error<'_, #err_ty>> },
                true,
            )
        }
        _ => (
            ty_def(api_bundle, return_ty, true, TyPos::Arg).expect("the trait has it"),
            false,
        ),
    }
}

/// The pump of the stream `f`: a number from its handler's name, with the element's indices.
fn pump_id(level: &LevelForwards<'_>, f: &LocalFn, own: bool) -> TokenStream {
    let name = level.prefixed(&level.local.level.items[f.item].ident);
    let id = Literal::u64_suffixed(fnv1a(name.as_bytes()));
    let len = level.depth + usize::from(own);
    if len == 0 {
        quote! { wire_weaver_server::PumpId::new(#id) }
    } else {
        let indices = (0..len).map(|i| quote! { index_chain[#i].get().0 });
        quote! { wire_weaver_server::PumpId::indexed(#id, &[#(#indices),*]) }
    }
}

/// FNV-1a of `bytes`: a stream's pump number from its handler's name, the same in every build.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, b| {
        (hash ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The open, close and sideband handlers of a plain stream returning `return_ty` on its open, or of a `keys`
/// stream: the open adds the trait's items as the connection's pump, until their first error, each turned into the
/// wire's form by `to_wire`, and their end sends the stream's `Close`; the close removes it.
fn stream_handlers(
    api_bundle: &CodegenBundle,
    level: &LevelForwards<'_>,
    f: &LocalFn,
    cx_ty: &TokenStream,
    return_ty: Option<&TypeOwned>,
    to_wire: Option<TokenStream>,
) -> TokenStream {
    let item = &level.local.level.items[f.item];
    let items = match to_wire {
        Some(to_wire) => quote! { wire_weaver::forward::until_error_map(items, #to_wire) },
        None => quote! { wire_weaver::forward::until_error(items) },
    };
    let own = f.index.is_some();
    let pump_id = pump_id(level, f, own);
    let name = level.prefixed(&item.ident);
    let ident = Ident::new(&name, Span::call_site());
    let arg_defs = f.args.iter().map(|(n, t)| quote! { #n: #t });
    // the serializer is reached at the level, its functions take the element's index first
    let ser = level.serializer();
    let item_ident = Ident::new(&item.ident, Span::call_site());
    let end = Ident::new(&format!("{}_sideband", item.ident), Span::call_site());
    let own_index = if own {
        let position = level.depth;
        quote! { index_chain[#position].get().0, }
    } else {
        quote! {}
    };
    let chain = level.chain_def(own);
    let service = quote! { wire_weaver_server::Serves::service(self) };
    let fail = quote! { wire_weaver::RpcResult::Failed(wire_weaver::Failure::from(e)) };
    let reach = level.reach(service, &fail);
    let call = level.call(f, quote! { cx.call() }, &fail);
    // the stream's end closes the client's stream too
    let pump = quote! {
        let ser = #ser;
        wire_weaver_server::Serves::pumps(self).add_ending(
            #pump_id,
            #items,
            move |item, buf| ser.#item_ident(#own_index item, buf).map(<[u8]>::len),
            Some(move |buf: &mut [u8]| {
                #ser
                    .#end(#own_index ww_client_server::StreamSideband::Close, buf)
                    .map(<[u8]>::len)
            }),
        );
    };
    let (output, open) = match return_ty {
        None => (
            quote! { () },
            quote! {
                let opened = { #reach #call };
                match opened {
                    Ok(items) => {
                        #pump
                        wire_weaver::RpcResult::Ready(())
                    }
                    Err(e) => wire_weaver::RpcResult::Failed(e.into()),
                }
            },
        ),
        Some(return_ty) => {
            let (output, _) = handler_output(api_bundle, Some(return_ty));
            (
                output,
                quote! {
                    let opened = { #reach #call };
                    let (answer, items) = wire_weaver::forward::open_result(opened);
                    if let Some(items) = items {
                        #pump
                    }
                    answer
                },
            )
        }
    };
    let close = Ident::new(&format!("close_{name}"), Span::call_site());
    let sideband = Ident::new(&format!("sideband_{name}"), Span::call_site());
    let close_chain = if chain.is_empty() {
        quote! {}
    } else {
        quote! { #chain }
    };
    quote! {
        async fn #ident(&mut self, cx: #cx_ty, #chain #(#arg_defs),*) -> wire_weaver::RpcResult<#output> {
            #open
        }

        async fn #close(&mut self, _cx: #cx_ty, #close_chain) {
            wire_weaver_server::Serves::pumps(self).remove(#pump_id);
        }

        async fn #sideband(
            &mut self,
            _cx: #cx_ty,
            #chain
            _sideband: ww_client_server::StreamSideband,
        ) -> Option<ww_client_server::StreamSideband> {
            None
        }
    }
}

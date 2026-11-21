//! A server forwarding to the API's local trait (SRV-49): `ww_codegen!(api :: Api for Server, server = true,
//! serve_trait = "api::Name")` generates the handler of every item the local trait (CLT-57) expresses, each calling
//! the trait on the service `wire_weaver_server::Serves` gives, and the backend traits a host runtime calls.
//!
//! The handlers are the ones the dispatcher calls anyway (one dispatcher): calls and reads take `&self` and run side
//! by side, writes and stream opens take `&mut self` and run alone. A stream open registers the trait's item stream
//! as a pump of the connection, its close removes it. Items the trait leaves out are handlers written by hand beside
//! the generated ones.
use crate::codegen::CodegenBundle;
use crate::codegen::local_trait::{LocalFnKind, local_fns};
use crate::codegen::ty_def::{TyPos, ty_def};
use crate::transform::CratePaths;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::Path;
use ww_self::{ApiItemKindOwned, TypeOwned};

/// What [gen_serve_trait] needs besides the API.
pub struct ServeTraitConfig<'a> {
    /// The local trait, `api::Name`.
    pub trait_path: &'a Path,
    /// The server struct the handlers are implemented on.
    pub server: &'a Path,
    /// The medium of `Context`, `()` if None.
    pub medium: Option<&'a Path>,
    /// The API's `*_FULL_GID` const, the server's version.
    pub full_gid: TokenStream,
}

/// The forwarding handlers of `config.server` and its `WireWeaverAsyncApiBackend` and
/// `WireWeaverSharedApiBackend` implementations.
pub fn gen_serve_trait(
    api_bundle: &ww_self::ApiBundleOwned,
    crate_paths: &CratePaths,
    config: &ServeTraitConfig<'_>,
) -> TokenStream {
    let api_bundle = &CodegenBundle::new(api_bundle, crate_paths);
    // the handlers don't name the stream type, so both forms of the trait are served by the same code
    let (fns, _skipped) = local_fns(api_bundle, true);
    let medium = match config.medium {
        Some(m) => quote! { #m },
        None => quote! { () },
    };
    let trait_path = config.trait_path;
    let cx_ty = quote! { &mut wire_weaver::Context<'_, impl wire_weaver::EventOut, #medium> };
    let service = quote! {
        <<Self as wire_weaver_server::Serves>::Service as #trait_path>
    };
    let handlers = fns.iter().map(|f| {
        let item = &api_bundle.root.items[f.item];
        let ident = &f.ident;
        let arg_defs = f.args.iter().map(|(n, t)| quote! { #n: #t });
        let names = f.args.iter().map(|(n, _)| n).collect::<Vec<_>>();
        let call = quote! {
            #service::#ident(wire_weaver_server::Serves::service(self), cx.call(), #(#names),*).await
        };
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
                quote! {
                    async fn #ident(&self, cx: #cx_ty, #(#arg_defs),*) -> wire_weaver::RpcResult<#output> {
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
                let name = &item.ident;
                if f.kind == LocalFnKind::Read {
                    let getter = Ident::new(&format!("get_{name}"), Span::call_site());
                    quote! {
                        async fn #getter(&self, cx: #cx_ty) -> wire_weaver::GetResult<#value_ty, #err_ty> {
                            wire_weaver::forward::read(#call)
                        }
                    }
                } else {
                    let setter = Ident::new(&format!("set_{name}"), Span::call_site());
                    let forward = if write_err_ty.is_some() {
                        quote! { write_result }
                    } else {
                        quote! { write }
                    };
                    quote! {
                        async fn #setter(&mut self, cx: #cx_ty, #(#arg_defs),*) -> wire_weaver::SetResult<#err_ty> {
                            wire_weaver::forward::#forward(#call)
                        }
                    }
                }
            }
            LocalFnKind::Stream => stream_handlers(api_bundle, f.item, ident, &cx_ty, &call, &f.args),
        }
    });

    let server = config.server;
    let full_gid = &config.full_gid;
    quote! {
        impl #server {
            #(#handlers)*
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

/// The open, close and sideband handlers of a plain stream: the open adds the trait's items as the connection's
/// pump, until their first error; the close removes it.
fn stream_handlers(
    api_bundle: &CodegenBundle,
    item_index: usize,
    ident: &Ident,
    cx_ty: &TokenStream,
    call: &TokenStream,
    args: &[(Ident, TokenStream)],
) -> TokenStream {
    let item = &api_bundle.root.items[item_index];
    let stream = item.kind.as_stream_fn().expect("a stream");
    let id = item_index as u64;
    let pump_id = quote! { wire_weaver_server::PumpId(#id) };
    let arg_defs = args.iter().map(|(n, t)| quote! { #n: #t });
    let pump = quote! {
        wire_weaver_server::Serves::pumps(self).add(
            #pump_id,
            wire_weaver::forward::until_error(items),
            |item, buf| stream_data_ser().#ident(item, buf).map(<[u8]>::len),
        );
    };
    let (output, open) = match stream.return_ty {
        None => (
            quote! { () },
            quote! {
                match #call {
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
                    let (answer, items) = wire_weaver::forward::open_result(#call);
                    if let Some(items) = items {
                        #pump
                    }
                    answer
                },
            )
        }
    };
    let close = Ident::new(&format!("close_{ident}"), Span::call_site());
    let sideband = Ident::new(&format!("sideband_{ident}"), Span::call_site());
    quote! {
        async fn #ident(&mut self, cx: #cx_ty, #(#arg_defs),*) -> wire_weaver::RpcResult<#output> {
            #open
        }

        async fn #close(&mut self, _cx: #cx_ty) {
            wire_weaver_server::Serves::pumps(self).remove(#pump_id);
        }

        async fn #sideband(
            &mut self,
            _cx: #cx_ty,
            _sideband: ww_client_server::StreamSideband,
        ) -> Option<ww_client_server::StreamSideband> {
            None
        }
    }
}

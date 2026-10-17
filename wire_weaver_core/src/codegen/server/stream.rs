use crate::codegen::ty_def::ty_def;
use crate::codegen::util;
use crate::codegen::util::maybe_quote;
use crate::codegen::{index_chain::IndexChain, ty_def::TyPos};
use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use ww_self::{
    ApiBundleOwned, ApiItemKindOwned, ApiLevelOwned, Batching, Delivery, Multiplicity,
    PropertyAccess, TypeOwned,
};

pub(crate) fn stream_ser_methods_recursive(
    bundle: &ApiBundleOwned,
    level: &ApiLevelOwned,
    index_chain: IndexChain,
    no_alloc: bool,
    is_root: bool,
) -> TokenStream {
    let mut ts = TokenStream::new();
    let mut child_ts = TokenStream::new();
    let mut methods_ts = TokenStream::new();
    let maybe_index_chain_field = index_chain.struct_field_def();

    for item in &level.items {
        let mut index_chain = index_chain;
        let id = item.id;
        let is_array = matches!(item.multiplicity, Multiplicity::Array { .. });
        let let_index_chain = let_index_chain(index_chain, id.0, is_array);
        let maybe_index_arg = maybe_quote(is_array, quote! { index: u32, });

        if let ApiItemKindOwned::Trait { trait_idx, .. } = &item.kind {
            let child_level = bundle.get_trait(trait_idx.0).unwrap();
            let child_struct_name = stream_ser_struct_name(child_level, bundle);

            index_chain.increment_length();
            if is_array {
                index_chain.increment_length();
            }
            child_ts.extend(stream_ser_methods_recursive(
                bundle,
                child_level,
                index_chain,
                no_alloc,
                false,
            ));

            let level_entry_fn_name = Ident::new(item.ident.as_str(), Span::call_site());
            methods_ts.extend(quote! {
                pub fn #level_entry_fn_name(&self, #maybe_index_arg) -> #child_struct_name {
                    #let_index_chain
                    #child_struct_name {
                        index_chain,
                    }
                }
            });
        }
        if let Some((_, is_up, _)) = item.kind.as_stream() {
            methods_ts.extend(sideband_ser_methods(
                item.ident.as_str(),
                if is_up { "stream" } else { "sink" },
                &maybe_index_arg,
                &let_index_chain,
                is_array,
            ));
        }
        if let ApiItemKindOwned::FlowSink { ty, .. } = &item.kind {
            methods_ts.extend(max_item_bytes(bundle, item.ident.as_str(), ty));
        }
        // batched streams are sent by `poll_streams`, producers push items (API-41)
        if let Some(stream) = item.kind.as_stream_fn()
            && let Some(batching) = stream.batched
        {
            methods_ts.extend(batched_ser_methods(
                bundle,
                item.ident.as_str(),
                stream.ty,
                batching,
                no_alloc,
                is_array,
            ));
            continue;
        }
        // streams going out of the device, and observable properties, whose updates are sent as stream data
        let (ty, kind) = match &item.kind {
            ApiItemKindOwned::Stream { ty, is_up: true }
            | ApiItemKindOwned::StreamFn {
                ty,
                delivery: Delivery::Plain,
                ..
            } => (ty, "stream"),
            ApiItemKindOwned::StreamFn { ty, delivery, .. } => {
                methods_ts.extend(positioned_ser_methods(
                    bundle,
                    item.ident.as_str(),
                    ty,
                    *delivery,
                    no_alloc,
                    &maybe_index_arg,
                    &let_index_chain,
                    is_array,
                ));
                continue;
            }
            ApiItemKindOwned::Property {
                ty,
                access: PropertyAccess::ReadOnly { .. } | PropertyAccess::ReadWrite { .. },
                ..
            } => (ty, "property"),
            _ => continue,
        };
        let lifetimes = if ty.is_lifetime(bundle).unwrap() {
            quote! { 'i, 'a }
        } else {
            quote! { 'a }
        };
        let send_lifetimes = maybe_quote(ty.is_lifetime(bundle).unwrap(), quote! { <'i> });

        let (value_ty, body) = if kind == "stream" && ty.is_byte_slice(bundle).unwrap() {
            let check_max_len = ty.max_len(bundle).unwrap().map(|max_len| {
                let max_len = max_len as usize;
                quote! {
                    if value.len() > #max_len {
                        return Err(ShrinkWrapError::MaxLenExceeded);
                    }
                }
            });
            (
                quote! { [u8] },
                quote! {
                    #check_max_len
                    let mut wr = BufWriter::new(scratch);
                    let event_builder = EventBuilder::new(0, &mut wr)?;
                    let event_kind_builder = EventKindBuilder::new(&mut wr)?;

                    #let_index_chain
                    let path = RefVec::Slice { slice: &index_chain };
                    wr.write(&path)?;
                    wr.write(&TailBytes(value))?;

                    event_kind_builder.finish_with_kind(EventKindDiscriminants::StreamData, &mut wr);
                    event_builder.finish(true, &mut wr);
                    wr.finish_and_take()
                },
            )
        } else {
            // `'i` of the fn: a `Series<'i, T<'i>>` needs one lifetime in both places (it is invariant over it)
            let ty_def = ty_def(bundle, ty, !no_alloc, TyPos::Def).unwrap();
            // The host reads the value from the event's tail bytes, as `from_ww_bytes` would: so it is written on its
            // own with `to_ww_bytes`, byte aligned and with its own sizes, between the event's header and the event's
            // sizes (the path length). Written in place, a value ending mid-byte lost its last bits, and its sizes
            // mixed with the event's.
            (
                quote! { #ty_def },
                quote! {
                    let mut header_buf = [0u8; 64];
                    let mut wr = BufWriter::new(&mut header_buf);
                    let event_builder = EventBuilder::new(0, &mut wr)?;
                    let event_kind_builder = EventKindBuilder::new(&mut wr)?;

                    #let_index_chain
                    let path = RefVec::Slice { slice: &index_chain };
                    wr.write(&path)?;

                    event_kind_builder.finish_with_kind(EventKindDiscriminants::StreamData, &mut wr);
                    event_builder.finish(true, &mut wr);
                    wr.align_byte();
                    let head_len = wr.pos().0;
                    let header = wr.finish_and_take()?;
                    if head_len > scratch.len() {
                        return Err(ShrinkWrapError::OutOfBoundsWriteRawSlice);
                    }
                    let value_len = value.to_ww_bytes(&mut scratch[head_len..])?.len();
                    let sizes = &header[head_len..];
                    let end = head_len + value_len + sizes.len();
                    if end > scratch.len() {
                        return Err(ShrinkWrapError::OutOfBoundsWriteRawSlice);
                    }
                    scratch[..head_len].copy_from_slice(&header[..head_len]);
                    scratch[head_len + value_len..end].copy_from_slice(sizes);
                    Ok(&scratch[..end])
                },
            )
        };
        let ident = Ident::new(item.ident.as_str(), Span::call_site());
        let send = Ident::new(format!("{}_send", item.ident).as_str(), Span::call_site());
        let send_blocking = Ident::new(
            format!("{}_send_blocking", item.ident).as_str(),
            Span::call_site(),
        );
        let maybe_index = maybe_quote(is_array, quote! { index, });
        let ser_doc = format!(
            "Serialize {kind} value, put it's bytes into Event with StreamData kind and serialize it"
        );
        let send_doc = format!(
            "Serialize {kind} value as StreamData event and send it, from a handler (pass `cx`) or from an event loop"
        );
        methods_ts.extend(quote! {
            #[doc = #ser_doc]
            pub fn #ident<#lifetimes>(
                &self,
                #maybe_index_arg
                value: & #value_ty,
                scratch: &'a mut [u8],
            ) -> Result<&'a [u8], ShrinkWrapError> {
                #body
            }

            #[doc = #send_doc]
            pub async fn #send #send_lifetimes(
                &self,
                #maybe_index_arg
                value: & #value_ty,
                out: &mut impl wire_weaver::EventOut,
            ) -> Result<(), wire_weaver::SendError> {
                wire_weaver::EventOut::send_with(out, |scratch| self.#ident(#maybe_index value, scratch).map(|bytes| bytes.len())).await
            }

            #[doc = #send_doc]
            pub fn #send_blocking #send_lifetimes(
                &self,
                #maybe_index_arg
                value: & #value_ty,
                out: &mut impl wire_weaver::BlockingEventOut,
            ) -> Result<(), wire_weaver::SendError> {
                wire_weaver::BlockingEventOut::send_with_blocking(out, |scratch| self.#ident(#maybe_index value, scratch).map(|bytes| bytes.len()))
            }
        });
    }

    let ser_struct_name = stream_ser_struct_name(level, bundle);
    let root_entry_fn = maybe_quote(
        is_root,
        quote! {
            /// Serializers of stream updates and property change notifications (sent as stream data on the property's path).
            /// The property ones are superseded by observe requests (`+observe`, `poll_properties`, SRV-18).
            pub const fn stream_data_ser() -> #ser_struct_name {
                #ser_struct_name {}
            }
        },
    );
    ts.extend(quote! {
        #root_entry_fn

        pub struct #ser_struct_name {
            #maybe_index_chain_field
        }

        impl #ser_struct_name {
            #methods_ts
        }

        #child_ts
    });
    ts
}

/// Serializers of a counted or reliable stream's events (API-30, API-31). Counted: `<name>_send` asks the stream's
/// `Counted` for the head and sends right away. Reliable: `<name>_push` puts the item into the stream's store,
/// `poll_streams` on the server sends it; `<name>_event` serializes an event of either.
#[allow(clippy::too_many_arguments)]
fn positioned_ser_methods(
    bundle: &ApiBundleOwned,
    ident: &str,
    ty: &TypeOwned,
    delivery: Delivery,
    no_alloc: bool,
    maybe_index_arg: &TokenStream,
    let_index_chain: &TokenStream,
    is_array: bool,
) -> TokenStream {
    let is_bytes = ty.is_byte_slice(bundle).unwrap();
    let is_lifetime = ty.is_lifetime(bundle).unwrap();
    let maybe_index = maybe_quote(is_array, quote! { index, });
    let check_max_len = match ty.max_len(bundle).unwrap() {
        Some(max_len) if is_bytes => {
            let max_len = max_len as usize;
            quote! {
                if value.len() > #max_len {
                    return Err(ShrinkWrapError::MaxLenExceeded);
                }
            }
        }
        _ => quote! {},
    };
    let (value_ty, write_item) = if is_bytes {
        (
            quote! { [u8] },
            quote! {
                |buf: &mut [u8]| {
                    #check_max_len
                    if value.len() > buf.len() {
                        return Err(ShrinkWrapError::OutOfBoundsWriteRawSlice);
                    }
                    buf[..value.len()].copy_from_slice(value);
                    Ok(value.len())
                }
            },
        )
    } else {
        // `'i` of the fn, as above
        let ty_def = ty_def(bundle, ty, !no_alloc, TyPos::Def).unwrap();
        (
            ty_def,
            quote! { |buf: &mut [u8]| value.to_ww_bytes(buf).map(|bytes| bytes.len()) },
        )
    };
    let value_lifetime = maybe_quote(is_lifetime, quote! { 'i, });
    let event = Ident::new(&format!("{ident}_event"), Span::call_site());
    let event_doc = format!(
        "Serialize a StreamItem event of `{ident}` with `head` and the item's bytes (as a store keeps them)"
    );
    let mut ts = quote! {
        #[doc = #event_doc]
        pub fn #event<'a>(
            &self,
            #maybe_index_arg
            head: &ww_client_server::stream::StreamHead,
            item: &[u8],
            scratch: &'a mut [u8],
        ) -> Result<&'a [u8], ShrinkWrapError> {
            #let_index_chain
            ww_client_server::stream::ser_stream_item_event(scratch, &index_chain, head, |buf: &mut [u8]| {
                if item.len() > buf.len() {
                    return Err(ShrinkWrapError::OutOfBoundsWriteRawSlice);
                }
                buf[..item.len()].copy_from_slice(item);
                Ok(item.len())
            })
        }
    };
    match delivery {
        Delivery::Plain => {}
        Delivery::Counted => {
            let ser = Ident::new(ident, Span::call_site());
            let send = Ident::new(&format!("{ident}_send"), Span::call_site());
            let send_blocking = Ident::new(&format!("{ident}_send_blocking"), Span::call_site());
            let send_doc = format!(
                "Send `value` of `units` units made at `t` as the next event of the counted stream `{ident}`: nothing \
                 is sent and the units are counted as lost while no host has it open, or when sending fails."
            );
            ts.extend(quote! {
                /// Serialize a StreamItem event with `head` and `value`.
                pub fn #ser<#value_lifetime 'a>(
                    &self,
                    #maybe_index_arg
                    head: &ww_client_server::stream::StreamHead,
                    value: & #value_ty,
                    scratch: &'a mut [u8],
                ) -> Result<&'a [u8], ShrinkWrapError> {
                    #let_index_chain
                    ww_client_server::stream::ser_stream_item_event(scratch, &index_chain, head, #write_item)
                }

                #[doc = #send_doc]
                pub async fn #send<#value_lifetime>(
                    &self,
                    #maybe_index_arg
                    stream: &mut ww_stream::tx::Counted,
                    value: & #value_ty,
                    units: u64,
                    t: Option<ww_stream::Second>,
                    out: &mut impl wire_weaver::EventOut,
                ) -> Result<(), wire_weaver::SendError> {
                    let Some(head) = stream.head(units, t) else {
                        return Ok(());
                    };
                    let r = wire_weaver::EventOut::send_with(out, |scratch| self.#ser(#maybe_index &head, value, scratch).map(|bytes| bytes.len())).await;
                    if r.is_err() {
                        stream.unsent(&head);
                    }
                    r
                }

                #[doc = #send_doc]
                pub fn #send_blocking<#value_lifetime>(
                    &self,
                    #maybe_index_arg
                    stream: &mut ww_stream::tx::Counted,
                    value: & #value_ty,
                    units: u64,
                    t: Option<ww_stream::Second>,
                    out: &mut impl wire_weaver::BlockingEventOut,
                ) -> Result<(), wire_weaver::SendError> {
                    let Some(head) = stream.head(units, t) else {
                        return Ok(());
                    };
                    let r = wire_weaver::BlockingEventOut::send_with_blocking(out, |scratch| self.#ser(#maybe_index &head, value, scratch).map(|bytes| bytes.len()));
                    if r.is_err() {
                        stream.unsent(&head);
                    }
                    r
                }
            });
        }
        Delivery::Reliable => {
            let push = Ident::new(&format!("{ident}_push"), Span::call_site());
            let push_doc = format!(
                "Put `value` of `units` units made at `t` into the store of the reliable stream `{ident}`, host or not \
                 (never waits for the link); the server's `poll_streams` sends it. `scratch` holds the serialized \
                 item on the way. A full store counts the units as lost (`Pushed::Lost`)."
            );
            let item = match ty.max_len(bundle).unwrap() {
                Some(max_len) if is_bytes => {
                    let max_len = max_len as usize;
                    quote! {
                        let _ = scratch;
                        if value.len() > #max_len {
                            return Err(ww_stream::tx::PushItemError::Serialize(ShrinkWrapError::MaxLenExceeded));
                        }
                        let item: &[u8] = value;
                    }
                }
                _ if is_bytes => quote! {
                    let _ = scratch;
                    let item: &[u8] = value;
                },
                _ => quote! {
                    let item = value.to_ww_bytes(scratch).map_err(ww_stream::tx::PushItemError::Serialize)?;
                },
            };
            ts.extend(quote! {
                #[doc = #push_doc]
                pub async fn #push<#value_lifetime S: ww_stream::store::StreamStore, const K: usize>(
                    &self,
                    stream: &mut ww_stream::tx::Reliable<S, K>,
                    value: & #value_ty,
                    units: u64,
                    t: Option<ww_stream::Second>,
                    scratch: &mut [u8],
                ) -> Result<ww_stream::tx::Pushed, ww_stream::tx::PushItemError<S::Error>> {
                    #item
                    stream.push(item, units, t).await.map_err(ww_stream::tx::PushItemError::Store)
                }
            });
        }
    }
    ts
}

/// Producer side of a batched stream (API-41): `<name>_push` puts one item into its batches, `<name>_push_many`
/// several; both O(1) per item, never encoding (`poll_streams` encodes and sends), so they may run in an interrupt.
/// They take anything that can be pushed into: the stream's `Batched` state when it owns its batcher, the batcher,
/// or its `Producer` half.
fn batched_ser_methods(
    bundle: &ApiBundleOwned,
    ident: &str,
    ty: &TypeOwned,
    batching: Batching,
    no_alloc: bool,
    is_array: bool,
) -> TokenStream {
    if is_array {
        // poll_streams refuses it with an error naming it
        return quote! {};
    }
    if ty.is_lifetime(bundle).unwrap() || ty.is_byte_slice(bundle).unwrap() {
        let e = format!(
            "batched stream {ident}: its item type borrows (bytes, a string, a vector): a batch needs items of a \
             bounded size"
        );
        return quote! { compile_error!(#e); };
    }
    let value_ty = ty_def(bundle, ty, !no_alloc, TyPos::Def).unwrap();
    let push = Ident::new(&format!("{ident}_push"), Span::call_site());
    let push_many = Ident::new(&format!("{ident}_push_many"), Span::call_site());
    match batching {
        Batching::Timed => {
            let push_doc = format!(
                "Put `value`, taken at device time `t`, into the batches of `{ident}`: a copy and two atomic \
                 operations, never an encode or a wait (an interrupt may call it). `poll_streams` sends the \
                 batches. Returns whether the batch was sealed, or lost because the event loop didn't keep up."
            );
            let push_many_doc = format!(
                "`{ident}_push` of every `(t, value)`: returns the last seal and the items lost on the way."
            );
            quote! {
                #[doc = #push_doc]
                #[inline]
                pub fn #push(
                    &self,
                    to: &mut impl ww_stream::batch::Push<ww_stream::batch::Timed<#value_ty>>,
                    value: #value_ty,
                    t: ww_stream::Second,
                ) -> ww_stream::batch::Seal {
                    to.push(&ww_stream::batch::Timed::new(t, value))
                }

                #[doc = #push_many_doc]
                pub fn #push_many(
                    &self,
                    to: &mut impl ww_stream::batch::Push<ww_stream::batch::Timed<#value_ty>>,
                    items: impl IntoIterator<Item = (ww_stream::Second, #value_ty)>,
                ) -> (ww_stream::batch::Seal, usize) {
                    to.push_many(items.into_iter().map(|(t, value)| ww_stream::batch::Timed::new(t, value)))
                }
            }
        }
        Batching::Untimed => {
            let push_doc = format!(
                "Put `value` into the batches of `{ident}` (untimed: the item has its own time, or needs none): \
                 a copy and two atomic operations, never an encode or a wait (an interrupt may call it). \
                 `poll_streams` sends the batches. Returns whether the batch was sealed, or lost because the \
                 event loop didn't keep up."
            );
            let push_many_doc = format!(
                "`{ident}_push` of every value: returns the last seal and the items lost on the way."
            );
            quote! {
                #[doc = #push_doc]
                #[inline]
                pub fn #push(
                    &self,
                    to: &mut impl ww_stream::batch::Push<#value_ty>,
                    value: #value_ty,
                ) -> ww_stream::batch::Seal {
                    to.push(&value)
                }

                #[doc = #push_many_doc]
                pub fn #push_many(
                    &self,
                    to: &mut impl ww_stream::batch::Push<#value_ty>,
                    items: impl IntoIterator<Item = #value_ty>,
                ) -> (ww_stream::batch::Seal, usize) {
                    to.push_many(items)
                }
            }
        }
        Batching::Unknown(_) => {
            let e = format!(
                "batched stream {ident}: a form of batching this wire_weaver doesn't know, update it"
            );
            quote! { compile_error!(#e); }
        }
    }
}

/// Largest item of a sink with flow control in bytes, to check its receive buffer (`SinkCredit::sized_for`).
fn max_item_bytes(bundle: &ApiBundleOwned, ident: &str, ty: &TypeOwned) -> TokenStream {
    let const_name = Ident::new(
        format!("{}_MAX_ITEM_BYTES", ident.to_case(Case::UpperSnake)).as_str(),
        Span::call_site(),
    );
    let fn_name = Ident::new(
        format!("{ident}_max_item_bytes").as_str(),
        Span::call_site(),
    );
    let doc = format!(
        "Largest `{ident}` item in bytes (the serialized value of one write, what credit counts), the receive buffer \
         must hold at least this much (`SinkCredit::sized_for`). None when the type has no upper bound (see \
         `shrink_wrap::MaxLen`)."
    );
    let fn_doc = format!("Same as [Self::{const_name}].");
    let body = if ty.is_byte_slice(bundle).unwrap() {
        // sent as is, without a length: a bound is the byte count itself
        match ty.max_len(bundle).unwrap() {
            Some(max_len) => quote! { Some(#max_len) },
            None => quote! { None },
        }
    } else {
        let ty_def = ty_def(bundle, ty, false, TyPos::Annotation).unwrap();
        quote! {
            // what the device gets in a write: the value serialized as a root
            match wire_weaver::shrink_wrap::MaxSize::buffer_len_of(
                <#ty_def as wire_weaver::shrink_wrap::SerializeShrinkWrap>::MAX_SIZE,
            ) {
                Some(len) => Some(len as u32),
                None => None,
            }
        }
    };
    quote! {
        #[doc = #doc]
        pub const #const_name: Option<u32> = #body;

        #[doc = #fn_doc]
        pub const fn #fn_name(&self) -> Option<u32> {
            Self::#const_name
        }
    }
}

/// Sideband events sent by the device on its own, not as a reply to a sideband request from the host.
fn sideband_ser_methods(
    ident: &str,
    kind: &str,
    maybe_index_arg: &TokenStream,
    let_index_chain: &TokenStream,
    is_array: bool,
) -> TokenStream {
    let ser = Ident::new(format!("{ident}_sideband").as_str(), Span::call_site());
    let send = Ident::new(format!("{ident}_sideband_send").as_str(), Span::call_site());
    let send_blocking = Ident::new(
        format!("{ident}_sideband_send_blocking").as_str(),
        Span::call_site(),
    );
    let maybe_index = maybe_quote(is_array, quote! { index, });
    let ser_doc = format!("Serialize Event with StreamSideband kind for this {kind}");
    let send_doc = format!(
        "Send a StreamSideband event for this {kind} to the host, from a handler (pass `cx`) or from an event loop"
    );
    quote! {
        #[doc = #ser_doc]
        pub fn #ser<'a>(
            &self,
            #maybe_index_arg
            sideband: StreamSideband,
            scratch: &'a mut [u8],
        ) -> Result<&'a [u8], ShrinkWrapError> {
            #let_index_chain
            ser_ok_event(
                scratch,
                0,
                EventKind::StreamSideband {
                    path: RefVec::Slice { slice: &index_chain },
                    sideband,
                },
            )
        }

        #[doc = #send_doc]
        pub async fn #send(
            &self,
            #maybe_index_arg
            sideband: StreamSideband,
            out: &mut impl wire_weaver::EventOut,
        ) -> Result<(), wire_weaver::SendError> {
            wire_weaver::EventOut::send_with(out, |scratch| self.#ser(#maybe_index sideband, scratch).map(|bytes| bytes.len())).await
        }

        #[doc = #send_doc]
        pub fn #send_blocking(
            &self,
            #maybe_index_arg
            sideband: StreamSideband,
            out: &mut impl wire_weaver::BlockingEventOut,
        ) -> Result<(), wire_weaver::SendError> {
            wire_weaver::BlockingEventOut::send_with_blocking(out, |scratch| self.#ser(#maybe_index sideband, scratch).map(|bytes| bytes.len()))
        }
    }
}

fn let_index_chain(mut index_chain: IndexChain, id: u32, is_array: bool) -> TokenStream {
    match (index_chain.is_empty(), is_array) {
        (false, false) => index_chain.push_back(quote! { self. }, quote! { UNib32(#id) }),
        (false, true) => {
            let op1 = index_chain.push_back(quote! { self. }, quote! { UNib32(#id) });
            let op2 = index_chain.push_back(quote! {}, quote! { UNib32(index) });
            quote! { #op1 #op2 }
        }
        (true, false) => quote! { let index_chain = [UNib32(#id)]; },
        (true, true) => {
            quote! { let index_chain = [UNib32(#id), UNib32(index)]; }
        }
    }
}

fn stream_ser_struct_name(api_level: &ApiLevelOwned, api_bundle: &ApiBundleOwned) -> Ident {
    let mod_name = util::mod_name(api_level, api_bundle);
    Ident::new(
        format!("{}_stream_serializer", mod_name)
            .to_case(Case::Pascal)
            .as_str(),
        mod_name.span(),
    )
}

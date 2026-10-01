use crate::codegen::ty_def::ty_def;
use crate::codegen::util;
use crate::codegen::util::maybe_quote;
use crate::codegen::{index_chain::IndexChain, ty_def::TyPos};
use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use ww_self::{ApiBundleOwned, ApiItemKindOwned, ApiLevelOwned, Multiplicity, PropertyAccess};

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

        if let ApiItemKindOwned::Trait { trait_idx } = &item.kind {
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
        // streams going out of the device, and observable properties, whose updates are sent as stream data
        let (ty, kind) = match &item.kind {
            ApiItemKindOwned::Stream { ty, is_up: true } => (ty, "stream"),
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

        let (value_ty, value_ser) = if kind == "stream" && ty.is_byte_slice(bundle).unwrap() {
            (
                quote! { [u8] },
                quote! {
                    wr.write(&TailBytes(value))?;
                },
            )
        } else {
            let ty_def = ty_def(bundle, ty, !no_alloc, TyPos::Arg).unwrap();
            let value_ser = quote! {
                value.ser_shrink_wrap(&mut wr)?;
            };

            (quote! { #ty_def }, value_ser)
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
                let mut wr = BufWriter::new(scratch);
                let event_builder = EventBuilder::new(0, &mut wr)?;
                let event_kind_builder = EventKindBuilder::new(&mut wr)?;

                #let_index_chain
                let path = RefVec::Slice { slice: &index_chain };
                wr.write(&path)?;

                #value_ser

                event_kind_builder.finish_with_kind(EventKindDiscriminants::StreamData, &mut wr);
                event_builder.finish(true, &mut wr);
                wr.finish_and_take()
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
            pub fn stream_data_ser() -> #ser_struct_name {
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

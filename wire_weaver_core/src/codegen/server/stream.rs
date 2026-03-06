use crate::codegen::index_chain::IndexChain;
use crate::codegen::ty_def::ty_def;
use crate::codegen::util;
use crate::codegen::util::maybe_quote;
use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use ww_self::{ApiBundleOwned, ApiItemKindOwned, ApiLevelOwned, Multiplicity};

pub(crate) fn stream_ser_methods_recursive(
    bundle: &ApiBundleOwned,
    level: &ApiLevelOwned,
    index_chain: IndexChain,
    crate_name: &str,
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
            let crate_name = child_level.crate_name(bundle).unwrap();
            let child_struct_name = stream_ser_struct_name(crate_name, child_level);

            index_chain.increment_length();
            if is_array {
                index_chain.increment_length();
            }
            child_ts.extend(stream_ser_methods_recursive(
                bundle,
                child_level,
                index_chain,
                crate_name,
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
        let ApiItemKindOwned::Stream { ty, is_up } = &item.kind else {
            continue;
        };
        if !*is_up {
            continue;
        }
        let lifetimes = if ty.is_unsized(bundle).unwrap() {
            quote! { 'i, 'a }
        } else {
            quote! { 'a }
        };

        let bytes_to_container = if no_alloc {
            quote! { RefVec::Slice { slice: value_bytes } }
        } else {
            quote! { Vec::from(value_bytes) }
        };

        let (value_ty, value_ser) = if ty.is_byte_slice(bundle).unwrap() {
            (quote! { [u8] }, quote! { let value_bytes = value; })
        } else {
            let ty_def = ty_def(bundle, ty, !no_alloc, true).unwrap();
            let value_ser = quote! {
                let mut wr = BufWriter::new(scratch_value);
                value.ser_shrink_wrap(&mut wr)?;
                let value_bytes = wr.finish_and_take()?;
            };

            (quote! { #ty_def }, value_ser)
        };
        let ident = Ident::new(item.ident.as_str(), Span::call_site());
        methods_ts.extend(quote! {
            #[doc = "Serialize stream value, put it's bytes into Event with StreamUpdate kind and serialize it"]
            pub fn #ident<#lifetimes>(
                &self,
                #maybe_index_arg
                value: & #value_ty,
                scratch_value: &mut [u8],
                scratch_event: &'a mut [u8]
            ) -> Result<&'a [u8], ShrinkWrapError> {
                #value_ser

                let mut wr = BufWriter::new(scratch_event);
                let data = #bytes_to_container;
                #let_index_chain
                let path = RefVec::Slice { slice: &index_chain };
                let event = Event {
                    seq: 0,
                    result: Ok(EventKind::StreamData { path, data })
                };
                event.ser_shrink_wrap(&mut wr)?;
                wr.finish_and_take()
            }
        });
    }

    let ser_struct_name = stream_ser_struct_name(crate_name, level);
    let root_entry_fn = maybe_quote(
        is_root,
        quote! {
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

fn stream_ser_struct_name(crate_name: &str, api_level: &ApiLevelOwned) -> Ident {
    let mod_name = util::mod_name(crate_name, api_level);
    Ident::new(
        format!("{}_stream_serializer", mod_name)
            .to_case(Case::Pascal)
            .as_str(),
        mod_name.span(),
    )
}

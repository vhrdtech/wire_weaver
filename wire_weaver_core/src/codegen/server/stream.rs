use crate::ast::api::{ApiItemKind, ApiLevel, Multiplicity};
use crate::codegen::index_chain::IndexChain;
use crate::codegen::util::maybe_quote;
use proc_macro2::{Ident, TokenStream};
use quote::quote;

pub(crate) fn stream_ser_methods_recursive(
    level: &ApiLevel,
    index_chain: IndexChain,
    ext_crate_name: Option<&Ident>,
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
        let let_index_chain = let_index_chain(index_chain, id, is_array);
        let maybe_index_arg = maybe_quote(is_array, quote! { index: u32, });

        if let ApiItemKind::ImplTrait {
            args,
            level: child_level,
        } = &item.kind
        {
            let child_level = child_level.as_ref().expect("non-empty level");
            let ext_crate_name = args.location.crate_name().clone();
            let child_struct_name = child_level.stream_ser_struct_name(ext_crate_name.as_ref());

            index_chain.increment_length();
            if is_array {
                index_chain.increment_length();
            }
            child_ts.extend(stream_ser_methods_recursive(
                child_level,
                index_chain,
                args.location.crate_name().as_ref(),
                no_alloc,
                false,
            ));

            let level_entry_fn_name = &args.resource_name;
            methods_ts.extend(quote! {
                pub fn #level_entry_fn_name(&self, #maybe_index_arg) -> #child_struct_name {
                    #let_index_chain
                    #child_struct_name {
                        index_chain,
                    }
                }
            });
        }
        let ApiItemKind::Stream { ident, ty, is_up } = &item.kind else {
            continue;
        };
        if !*is_up {
            continue;
        }
        let lifetimes = if ty.potential_lifetimes() {
            quote! { 'i, 'a }
        } else {
            quote! { 'a }
        };
        let ty = ty.def(no_alloc);

        let bytes_to_container = if no_alloc {
            quote! { RefVec::Slice { slice: value_bytes } }
        } else {
            quote! { Vec::from(value_bytes) }
        };

        methods_ts.extend(quote! {
            #[doc = "Serialize stream value, put it's bytes into Event with StreamUpdate kind and serialize it"]
            pub fn #ident<#lifetimes>(&self, #maybe_index_arg value: &#ty, scratch_value: &mut [u8], scratch_event: &'a mut [u8]) -> Result<&'a [u8], ShrinkWrapError> {
                let mut wr = BufWriter::new(scratch_value);
                value.ser_shrink_wrap(&mut wr)?;
                let value_bytes = wr.finish_and_take()?;

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

    let ser_struct_name = level.stream_ser_struct_name(ext_crate_name);
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

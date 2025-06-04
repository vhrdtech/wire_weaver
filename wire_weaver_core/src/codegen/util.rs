use crate::ast::path::Path;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{ToTokens, quote};
use shrink_wrap::ElementSize;
use syn::{LitInt, LitStr};

pub(crate) fn serdes(
    ty_name: Ident,
    ser: impl ToTokens,
    des: impl ToTokens,
    lifetime: TokenStream,
    cfg: TokenStream,
    element_size: TokenStream,
) -> TokenStream {
    quote! {
        #cfg
        impl #lifetime SerializeShrinkWrap for #ty_name #lifetime {
            const ELEMENT_SIZE: ElementSize = #element_size;

            fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), ShrinkWrapError> {
                #ser
            }
        }

        #cfg
        impl<'i> DeserializeShrinkWrap<'i> for #ty_name #lifetime {
            const ELEMENT_SIZE: ElementSize = #element_size;

            fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, ShrinkWrapError> {
                #des
            }
        }
    }
}

pub(crate) fn strings_to_derive(traits: &Vec<Path>) -> TokenStream {
    if traits.is_empty() {
        quote! {}
    } else {
        // let traits = traits
        //     .iter()
        //     .map(|s| Ident::new(s.as_str(), Span::call_site()));
        quote! {
            #[derive(#(#traits),*)]
        }
    }
}

pub fn element_size_ts(size: ElementSize) -> TokenStream {
    match size {
        // ElementSize::Implied => quote! { ElementSize::Implied },
        ElementSize::Unsized => quote! { ElementSize::Unsized },
        ElementSize::UnsizedFinalStructure => quote! { ElementSize::UnsizedFinalStructure },
        ElementSize::SelfDescribing => quote! { ElementSize::SelfDescribing },
        ElementSize::Sized { size_bits } => {
            let size_bits = LitInt::new(format!("{size_bits}").as_str(), Span::call_site());
            quote! { ElementSize::Sized { size_bits: #size_bits } }
        }
    }
}

pub fn sum_element_sizes_recursively(first: ElementSize, sizes: Vec<Ident>) -> TokenStream {
    let first = element_size_ts(first);
    if sizes.is_empty() {
        quote! { #first }
    } else {
        let sizes = sum_unknown(sizes);
        quote! { #first.add(#sizes) }
    }
}

fn sum_unknown(mut sizes: Vec<Ident>) -> TokenStream {
    if let Some(ident) = sizes.pop() {
        let inner = sum_unknown(sizes);
        if inner.is_empty() {
            quote! { <#ident as SerializeShrinkWrap>::ELEMENT_SIZE }
        } else {
            quote! { <#ident as SerializeShrinkWrap>::ELEMENT_SIZE.add(#inner) }
        }
    } else {
        TokenStream::new()
    }
}

pub fn assert_element_size(
    ident: &crate::ast::ident::Ident,
    size: ElementSize,
    cfg: Option<LitStr>,
) -> TokenStream {
    let size_ts = match size {
        ElementSize::Unsized => quote! { Unsized },
        ElementSize::UnsizedFinalStructure => quote! { UnsizedFinalStructure },
        ElementSize::SelfDescribing => quote! { SelfDescribing },
        ElementSize::Sized { .. } => quote! { Sized { .. } },
    };
    let size = match size {
        ElementSize::Unsized => "Unsized",
        ElementSize::UnsizedFinalStructure => "UnsizedFinalStructure",
        ElementSize::SelfDescribing => "SelfDescribing",
        ElementSize::Sized { .. } => "Sized",
    };
    let err_msg = format!("{} must be {size}", ident.sym);
    let err_msg = LitStr::new(&err_msg, Span::call_site());
    let cfg = if let Some(cfg) = cfg {
        quote! { #[cfg(feature = #cfg)] }
    } else {
        quote! {}
    };
    quote! {
        #cfg
        const _: () = assert!(
            matches!(<#ident as SerializeShrinkWrap>::ELEMENT_SIZE, ElementSize::#size_ts),
            #err_msg
        );

        #cfg
        const _: () = assert!(
            matches!(<#ident as DeserializeShrinkWrap>::ELEMENT_SIZE, ElementSize::#size_ts),
            #err_msg
        );
    }
}

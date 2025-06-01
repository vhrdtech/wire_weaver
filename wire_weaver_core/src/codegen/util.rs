use crate::ast::path::Path;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{ToTokens, quote};
use shrink_wrap::ElementSize;
use syn::LitInt;

pub(crate) fn serdes(
    ty_name: Ident,
    ser: impl ToTokens,
    des: impl ToTokens,
    lifetime: TokenStream,
    cfg: TokenStream,
    element_size: ElementSize,
) -> TokenStream {
    let element_size = match element_size {
        ElementSize::Unsized => quote! { Unsized },
        ElementSize::Sized { size_bits } => {
            let size_bits = LitInt::new(format!("{size_bits}").as_str(), Span::call_site());
            quote! { Sized { size_bits: #size_bits } }
        }
        ElementSize::UnsizedSelfDescribing => quote! { UnsizedSelfDescribing },
        // ElementSize::Implied => quote! { Implied },
    };
    quote! {
        #cfg
        impl #lifetime SerializeShrinkWrap for #ty_name #lifetime {
            const ELEMENT_SIZE: ElementSize = ElementSize::#element_size;

            fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), ShrinkWrapError> {
                #ser
            }
        }

        #cfg
        impl<'i> DeserializeShrinkWrap<'i> for #ty_name #lifetime {
            const ELEMENT_SIZE: ElementSize = ElementSize::#element_size;

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

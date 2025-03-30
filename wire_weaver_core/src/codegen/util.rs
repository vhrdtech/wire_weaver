use crate::ast::path::Path;
use proc_macro2::{Ident, TokenStream};
use quote::{ToTokens, quote};

pub(crate) fn serdes(
    ty_name: Ident,
    ser: impl ToTokens,
    des: impl ToTokens,
    lifetime: TokenStream,
) -> TokenStream {
    quote! {
        impl #lifetime SerializeShrinkWrap for #ty_name #lifetime {
            fn ser_shrink_wrap(
                &self,
                wr: &mut BufWriter
            ) -> Result<(), ShrinkWrapError> {
                #ser
            }
        }

        impl<'i> DeserializeShrinkWrap<'i> for #ty_name #lifetime {
            fn des_shrink_wrap<'di>(
                rd: &'di mut BufReader<'i>,
                _element_size: ElementSize
            ) -> Result<Self, ShrinkWrapError> {
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

use proc_macro2::{Ident, Span, TokenStream};
use quote::{quote, ToTokens};

pub(crate) fn serdes(
    ty_name: Ident,
    ser: impl ToTokens,
    des: impl ToTokens,
    lifetime: TokenStream,
) -> TokenStream {
    quote! {
        impl #lifetime wire_weaver::shrink_wrap::SerializeShrinkWrap for #ty_name #lifetime {
            fn ser_shrink_wrap(
                &self,
                wr: &mut wire_weaver::shrink_wrap::BufWriter
            ) -> Result<(), wire_weaver::shrink_wrap::Error> {
                #ser
            }
        }

        impl<'i> wire_weaver::shrink_wrap::DeserializeShrinkWrap<'i> for #ty_name #lifetime {
            fn des_shrink_wrap<'di>(
                rd: &'di mut wire_weaver::shrink_wrap::BufReader<'i>,
                _element_size: wire_weaver::shrink_wrap::ElementSize
            ) -> Result<Self, wire_weaver::shrink_wrap::Error> {
                #des
            }
        }
    }
}

pub(crate) fn strings_to_derive(traits: &Vec<String>) -> TokenStream {
    if traits.is_empty() {
        quote! {}
    } else {
        let traits = traits
            .iter()
            .map(|s| Ident::new(s.as_str(), Span::call_site()));
        quote! {
            #[derive(#(#traits),*)]
        }
    }
}

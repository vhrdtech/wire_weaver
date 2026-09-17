use proc_macro2::{Ident, TokenStream};
use quote::{ToTokens, quote};

pub(crate) fn serdes_scaffold(
    ty_name: &Ident,
    ser: impl ToTokens,
    des: impl ToTokens,
    cfg: Option<&TokenStream>,
    element_size: TokenStream,
    is_ref: bool,
) -> TokenStream {
    let (ser_trait, des_trait) = if is_ref {
        (
            quote! { SerializeShrinkWrap },
            quote! { DeserializeShrinkWrap },
        )
    } else {
        (
            quote! { SerializeShrinkWrapOwned },
            quote! { DeserializeShrinkWrapOwned },
        )
    };
    let lifetime = maybe_quote(is_ref, || quote! { <'i> });
    quote! {
        #cfg
        impl #lifetime #ser_trait for #ty_name #lifetime {
            const ELEMENT_SIZE: ElementSize = #element_size;

            fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), ShrinkWrapError> {
                #ser
            }
        }

        #cfg
        impl #lifetime #des_trait #lifetime for #ty_name #lifetime {
            const ELEMENT_SIZE: ElementSize = #element_size;

            fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, ShrinkWrapError> {
                #des
            }
        }
    }
}

pub(crate) fn maybe_quote<F: FnMut() -> TokenStream>(
    condition: bool,
    mut call_if_true: F,
) -> TokenStream {
    if condition {
        call_if_true()
    } else {
        TokenStream::new()
    }
}

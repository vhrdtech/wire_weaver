use proc_macro2::{Ident, TokenStream};
use quote::{ToTokens, quote};

pub(crate) fn serdes_scaffold(
    ty_name: &Ident,
    ser: impl ToTokens,
    des: impl ToTokens,
    cfg: Option<&TokenStream>,
    element_size: TokenStream,
    is_ref: bool,
    has_lifetime: bool,
) -> TokenStream {
    let cfg = cfg.map(|cond| quote! { #[cfg(#cond)] });
    if is_ref {
        // The trait itself always carries a `'i` (the input buffer's lifetime); `#ty_lifetime` is
        // only applied to `#ty_name` when the type actually has a `<'i>` generic of its own.
        let ty_lifetime = maybe_quote(has_lifetime, || quote! { <'i> });
        quote! {
            #cfg
            impl<'i> SerializeShrinkWrap for #ty_name #ty_lifetime {
                const ELEMENT_SIZE: ElementSize = #element_size;

                fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), ShrinkWrapError> {
                    #ser
                }
            }

            #cfg
            impl<'i> DeserializeShrinkWrap<'i> for #ty_name #ty_lifetime {
                const ELEMENT_SIZE: ElementSize = #element_size;

                fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, ShrinkWrapError> {
                    #des
                }
            }
        }
    } else {
        quote! {
            #cfg
            impl SerializeShrinkWrapOwned for #ty_name {
                const ELEMENT_SIZE: ElementSize = #element_size;

                fn ser_shrink_wrap_owned(&self, wr: &mut BufWriterOwned) -> Result<(), ShrinkWrapError> {
                    #ser
                }
            }

            #cfg
            impl DeserializeShrinkWrapOwned for #ty_name {
                const ELEMENT_SIZE: ElementSize = #element_size;

                fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, ShrinkWrapError> {
                    #des
                }
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

use crate::ast::ty::Type;
use proc_macro2::{Ident, TokenStream};
use quote::{ToTokens, quote};

/// How generated code names `shrink_wrap` items: `::shrink_wrap`, `::wire_weaver::shrink_wrap`, whatever
/// `crate_path(..)` says, or `None` to leave the names unqualified and rely on `use shrink_wrap::prelude::*`
/// in the user's module (when the crate can't be found in the user's `Cargo.toml`).
#[derive(Clone, Debug, Default)]
pub(crate) struct CratePath(pub(crate) Option<TokenStream>);

impl CratePath {
    /// `#cp::#name`, or `#name` unqualified.
    pub(crate) fn item(&self, name: TokenStream) -> TokenStream {
        match &self.0 {
            Some(cp) => quote! { #cp::#name },
            None => name,
        }
    }

    /// `shrink_wrap::Error`: `#cp::Error`, or the prelude's `ShrinkWrapError` alias when unqualified, so that it
    /// doesn't clash with the user's own `Error`.
    pub(crate) fn error(&self) -> TokenStream {
        match &self.0 {
            Some(cp) => quote! { #cp::Error },
            None => quote! { ShrinkWrapError },
        }
    }

    /// The (serialize, deserialize) traits for the borrowed (`is_ref`) or owned side.
    pub(crate) fn serdes_traits(&self, is_ref: bool) -> (TokenStream, TokenStream) {
        if is_ref {
            (
                self.item(quote! { SerializeShrinkWrap }),
                self.item(quote! { DeserializeShrinkWrap }),
            )
        } else {
            (
                self.item(quote! { SerializeShrinkWrapOwned }),
                self.item(quote! { DeserializeShrinkWrapOwned }),
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn serdes_scaffold(
    ty_name: &Ident,
    ser: impl ToTokens,
    des: impl ToTokens,
    cfg: Option<&TokenStream>,
    element_size: TokenStream,
    is_ref: bool,
    has_lifetime: bool,
    cp: &CratePath,
) -> TokenStream {
    let cfg = cfg.map(|cond| quote! { #[cfg(#cond)] });
    let element_size_ty = cp.item(quote! { ElementSize });
    let error = cp.error();
    let buf_reader = cp.item(quote! { BufReader });
    let (ser_trait, des_trait) = cp.serdes_traits(is_ref);
    if is_ref {
        let buf_writer = cp.item(quote! { BufWriter });
        // The trait itself always carries a `'i` (the input buffer's lifetime); `#ty_lifetime` is
        // only applied to `#ty_name` when the type actually has a `<'i>` generic of its own.
        let ty_lifetime = maybe_quote(has_lifetime, || quote! { <'i> });
        quote! {
            #cfg
            impl<'i> #ser_trait for #ty_name #ty_lifetime {
                const ELEMENT_SIZE: #element_size_ty = #element_size;

                fn ser_shrink_wrap(&self, wr: &mut #buf_writer) -> Result<(), #error> {
                    #ser
                }
            }

            #cfg
            impl<'i> #des_trait<'i> for #ty_name #ty_lifetime {
                const ELEMENT_SIZE: #element_size_ty = #element_size;

                fn des_shrink_wrap<'di>(rd: &'di mut #buf_reader<'i>) -> Result<Self, #error> {
                    #des
                }
            }
        }
    } else {
        let buf_writer = cp.item(quote! { BufWriterOwned });
        quote! {
            #cfg
            impl #ser_trait for #ty_name {
                const ELEMENT_SIZE: #element_size_ty = #element_size;

                fn ser_shrink_wrap_owned(&self, wr: &mut #buf_writer) -> Result<(), #error> {
                    #ser
                }
            }

            #cfg
            impl #des_trait for #ty_name {
                const ELEMENT_SIZE: #element_size_ty = #element_size;

                fn des_shrink_wrap_owned(rd: &mut #buf_reader<'_>) -> Result<Self, #error> {
                    #des
                }
            }
        }
    }
}

/// Const asserts that every user type in the fields before a `TailSize` slot is `Sized` or `SelfDescribing`:
/// what `check_tail_size_position` checks for known types. Empty when there is no slot or no user type before it.
pub(crate) fn assert_sized_before_tail_size(
    types: &[&Type],
    owner: &Ident,
    cfg: Option<&TokenStream>,
    is_ref: bool,
    cp: &CratePath,
) -> TokenStream {
    let Some(slot) = types.iter().position(|ty| matches!(ty, Type::TailSize(_))) else {
        return TokenStream::new();
    };
    let mut externals = vec![];
    for ty in &types[..slot] {
        ty.externals(&mut externals);
    }
    let cfg = cfg.map(|cond| quote! { #[cfg(#cond)] });
    let element_size = cp.item(quote! { ElementSize });
    let (ser_trait, _) = cp.serdes_traits(is_ref);
    let mut tokens = TokenStream::new();
    for (path, is_lifetime) in externals {
        let path = &path;
        let ty = maybe_quote(is_lifetime && is_ref, || quote! { <'static> });
        let err_msg = format!(
            "{}: field of type {} before the TailSize slot must be Sized or SelfDescribing",
            owner,
            quote! { #path }
        );
        tokens.extend(quote! {
            #cfg
            const _: () = assert!(
                matches!(
                    <#path #ty as #ser_trait>::ELEMENT_SIZE,
                    #element_size::Sized { .. } | #element_size::SelfDescribing
                ),
                #err_msg
            );
        });
    }
    tokens
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

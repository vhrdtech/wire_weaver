use crate::ast::generics::Generics;
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
    generics: &Generics,
    ser: impl ToTokens,
    des: impl ToTokens,
    cfg: Option<&TokenStream>,
    element_size: TokenStream,
    pushes_len: Option<TokenStream>,
    max_size: TokenStream,
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
        let max_size_ty = cp.item(quote! { MaxSize });
        // The trait itself always carries a `'i` (the input buffer's lifetime); `#ty_lifetime` is
        // only applied to `#ty_name` when the type actually has a `<'i>` generic of its own.
        let ty_lifetime = generics.ty(has_lifetime);
        // a type parameter's size is part of the type's, so both impls need it to be serializable
        let ser_params = generics.impl_params(Some(ser_trait.clone()));
        let des_params = generics.impl_params(Some(quote! { #ser_trait + #des_trait<'i> }));
        let where_clause = generics.where_clause(&[]);
        // `false` only when no field can push a length, see SerializeShrinkWrap::PUSHES_LEN
        let pushes_len = pushes_len.unwrap_or_else(|| quote! { false });
        quote! {
            #cfg
            impl<'i, #ser_params> #ser_trait for #ty_name #ty_lifetime #where_clause {
                const ELEMENT_SIZE: #element_size_ty = #element_size;
                const PUSHES_LEN: bool = #pushes_len;
                const MAX_SIZE: Option<#max_size_ty> = #max_size;

                fn ser_shrink_wrap(&self, wr: &mut #buf_writer) -> Result<(), #error> {
                    #ser
                }
            }

            #cfg
            impl<'i, #des_params> #des_trait<'i> for #ty_name #ty_lifetime #where_clause {
                const ELEMENT_SIZE: #element_size_ty = #element_size;

                fn des_shrink_wrap<'di>(rd: &'di mut #buf_reader<'i>) -> Result<Self, #error> {
                    #des
                }
            }
        }
    } else {
        let buf_writer = cp.item(quote! { BufWriterOwned });
        let ty_params = generics.ty(false);
        let ser_params = generics.impl_params(Some(ser_trait.clone()));
        let des_params = generics.impl_params(Some(quote! { #ser_trait + #des_trait }));
        let where_clause = generics.where_clause(&[]);
        quote! {
            #cfg
            impl<#ser_params> #ser_trait for #ty_name #ty_params #where_clause {
                const ELEMENT_SIZE: #element_size_ty = #element_size;

                fn ser_shrink_wrap_owned(&self, wr: &mut #buf_writer) -> Result<(), #error> {
                    #ser
                }
            }

            #cfg
            impl<#des_params> #des_trait for #ty_name #ty_params #where_clause {
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
        let ty = path.tokens((is_lifetime && is_ref).then(|| quote! { 'static }));
        let err_msg = format!(
            "{}: field of type {} before the TailSize slot must be Sized or SelfDescribing",
            owner, ty
        );
        tokens.extend(quote! {
            #cfg
            const _: () = assert!(
                matches!(
                    <#ty as #ser_trait>::ELEMENT_SIZE,
                    #element_size::Sized { .. } | #element_size::SelfDescribing
                ),
                #err_msg
            );
        });
    }
    tokens
}

/// DER-12: rewrites bare positional placeholders (`{0}`, `{1:#x}`, ...) in a `display` message into `{_0}`,
/// `{_1:#x}`, so a tuple variant's fields - bound as ordinary named locals `_0`, `_1`, ... - are picked up by
/// Rust's implicit-named-argument capture, the same way `{field}` picks up a named field. This way `write!`
/// is never given an explicit argument list, so a field the message doesn't mention is merely an unused local
/// (silenced with `#[allow(unused_variables)]`) instead of a hard "argument never used" error.
pub(crate) fn rewrite_positional_placeholders(fmt: &str) -> String {
    let mut out = String::with_capacity(fmt.len());
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push_str("{{");
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                out.push_str("}}");
            }
            '{' => {
                out.push('{');
                let mut name = String::new();
                while let Some(&next) = chars.peek() {
                    if next == '}' || next == ':' {
                        break;
                    }
                    name.push(next);
                    chars.next();
                }
                if !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit()) {
                    out.push('_');
                }
                out.push_str(&name);
            }
            c => out.push(c),
        }
    }
    out
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

#[cfg(test)]
mod rewrite_positional_placeholders_tests {
    use super::rewrite_positional_placeholders;

    #[test]
    fn renames_positional_placeholders_only() {
        assert_eq!(rewrite_positional_placeholders("Timed out."), "Timed out.");
        assert_eq!(
            rewrite_positional_placeholders("In use by {by}."),
            "In use by {by}."
        );
        assert_eq!(
            rewrite_positional_placeholders("Hardware fault {0:#x}."),
            "Hardware fault {_0:#x}."
        );
        assert_eq!(
            rewrite_positional_placeholders("{0} and {1}"),
            "{_0} and {_1}"
        );
    }

    #[test]
    fn leaves_escaped_braces_alone() {
        assert_eq!(rewrite_positional_placeholders("{{0}}"), "{{0}}");
        assert_eq!(
            rewrite_positional_placeholders("{{literal}} {0}"),
            "{{literal}} {_0}"
        );
    }
}

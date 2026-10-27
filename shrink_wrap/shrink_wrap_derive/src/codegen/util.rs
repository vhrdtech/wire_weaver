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
    // bodies of `ser_shrink_wrap_latch` and `des_shrink_wrap_latch` (`_owned_latch`), when not the default
    latch: Option<(TokenStream, TokenStream)>,
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
        let (ser_latch, des_latch) = latch
            .map(|(ser, des)| {
                (
                    quote! {
                        #[inline(always)]
                        fn ser_shrink_wrap_latch(&self, wr: &mut #buf_writer) -> Result<(), #error> {
                            #ser
                        }
                    },
                    quote! {
                        #[inline(always)]
                        fn des_shrink_wrap_latch(rd: &mut #buf_reader<'i>) -> Result<Self, #error> {
                            #des
                        }
                    },
                )
            })
            .unzip();
        quote! {
            #cfg
            impl<'i, #ser_params> #ser_trait for #ty_name #ty_lifetime #where_clause {
                const ELEMENT_SIZE: #element_size_ty = #element_size;
                const PUSHES_LEN: bool = #pushes_len;
                const MAX_SIZE: Option<#max_size_ty> = #max_size;

                fn ser_shrink_wrap(&self, wr: &mut #buf_writer) -> Result<(), #error> {
                    #ser
                }

                #ser_latch
            }

            #cfg
            impl<'i, #des_params> #des_trait<'i> for #ty_name #ty_lifetime #where_clause {
                const ELEMENT_SIZE: #element_size_ty = #element_size;

                fn des_shrink_wrap<'di>(rd: &'di mut #buf_reader<'i>) -> Result<Self, #error> {
                    #des
                }

                #des_latch
            }
        }
    } else {
        let buf_writer = cp.item(quote! { BufWriterOwned });
        let ty_params = generics.ty(false);
        let ser_params = generics.impl_params(Some(ser_trait.clone()));
        let des_params = generics.impl_params(Some(quote! { #ser_trait + #des_trait }));
        let where_clause = generics.where_clause(&[]);
        let (ser_latch, des_latch) = latch
            .map(|(ser, des)| {
                (
                    quote! {
                        #[inline(always)]
                        fn ser_shrink_wrap_owned_latch(&self, wr: &mut #buf_writer) -> Result<(), #error> {
                            #ser
                        }
                    },
                    quote! {
                        #[inline(always)]
                        fn des_shrink_wrap_owned_latch(rd: &mut #buf_reader<'_>) -> Result<Self, #error> {
                            #des
                        }
                    },
                )
            })
            .unzip();
        quote! {
            #cfg
            impl<#ser_params> #ser_trait for #ty_name #ty_params #where_clause {
                const ELEMENT_SIZE: #element_size_ty = #element_size;

                fn ser_shrink_wrap_owned(&self, wr: &mut #buf_writer) -> Result<(), #error> {
                    #ser
                }

                #ser_latch
            }

            #cfg
            impl<#des_params> #des_trait for #ty_name #ty_params #where_clause {
                const ELEMENT_SIZE: #element_size_ty = #element_size;

                fn des_shrink_wrap_owned(rd: &mut #buf_reader<'_>) -> Result<Self, #error> {
                    #des
                }

                #des_latch
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

/// DER-17: a type that derives `serde::Deserialize` gives its `#[default = ..]` fields `#[serde(default = ..)]`, so
/// serde data written before such a field was added (a saved RON or JSON file) reads it as its default, as
/// shrink_wrap data does. The attribute names a hidden function `Type::__ww_default_[<variant>_]<field>`.
pub(crate) struct SerdeDefaults {
    /// `None` when the type always derives it, else the condition of each `cfg_attr(cond, ..)` that does.
    conds: Option<Vec<TokenStream>>,
    /// `Type::<T>`: inside serde's derive `Self` is its visitor, so the functions are named by the type's path.
    owner: String,
}

impl SerdeDefaults {
    /// `None` when the type doesn't derive `serde::Deserialize`: through a `derive(..)` path ending in
    /// `Deserialize`, or a `cfg_attr(..)` mentioning it.
    pub(crate) fn new(
        cfg_attr: &[TokenStream],
        derive: &[syn::Path],
        ident: &Ident,
        generics: &Generics,
    ) -> Option<Self> {
        let conds = if derive
            .iter()
            .any(|p| p.segments.last().is_some_and(|s| s.ident == "Deserialize"))
        {
            None
        } else {
            let conds: Vec<TokenStream> = cfg_attr
                .iter()
                .filter_map(|ts| {
                    let mut tokens = ts.clone().into_iter();
                    let cond: TokenStream = tokens
                        .by_ref()
                        .take_while(
                            |t| !matches!(t, proc_macro2::TokenTree::Punct(p) if p.as_char() == ','),
                        )
                        .collect();
                    mentions_deserialize(tokens.collect()).then_some(cond)
                })
                .collect();
            if conds.is_empty() {
                return None;
            }
            Some(conds)
        };
        let ty_generics = generics.ty(false);
        let owner = if ty_generics.is_empty() {
            ident.to_string()
        } else {
            format!("{ident}::{ty_generics}")
        };
        Some(SerdeDefaults { conds, owner })
    }

    /// `#[serde(default = "Type::<fn_name>")]`, under the conditions that derive `Deserialize`.
    pub(crate) fn field_attr(&self, fn_name: &Ident) -> TokenStream {
        let path = syn::LitStr::new(
            &format!("{}::{fn_name}", self.owner),
            proc_macro2::Span::call_site(),
        );
        match &self.conds {
            None => quote! { #[serde(default = #path)] },
            Some(conds) => quote! { #(#[cfg_attr(#conds, serde(default = #path))])* },
        }
    }

    /// `__ww_default_[<variant>_]<field>`
    pub(crate) fn fn_name(variant: Option<&Ident>, field: &Ident) -> Ident {
        let field = field.to_string();
        let field = field.trim_start_matches("r#");
        match variant {
            Some(variant) => quote::format_ident!("__ww_default_{variant}_{field}"),
            None => quote::format_ident!("__ww_default_{field}"),
        }
    }

    /// `fn <fn_name>() -> <ty> { <default> }`
    pub(crate) fn fn_def(fn_name: &Ident, ty: TokenStream, default: &syn::Expr) -> TokenStream {
        quote! {
            #[doc(hidden)]
            #[allow(dead_code, non_snake_case)]
            fn #fn_name() -> #ty { #default }
        }
    }

    /// The inherent impl holding `fns`, empty without any.
    pub(crate) fn impl_block(
        fns: Vec<TokenStream>,
        cfg: Option<&TokenStream>,
        ident: &Ident,
        generics: &Generics,
        lifetime: bool,
    ) -> TokenStream {
        if fns.is_empty() {
            return quote! {};
        }
        let cfg = cfg.map(|cond| quote! { #[cfg(#cond)] });
        let params = generics.impl_params(None);
        let ty_generics = generics.ty(lifetime);
        let where_clause = generics.where_clause(&[]);
        quote! {
            #cfg
            impl<'i, #params> #ident #ty_generics #where_clause {
                #(#fns)*
            }
        }
    }
}

fn mentions_deserialize(ts: TokenStream) -> bool {
    ts.into_iter().any(|t| match t {
        proc_macro2::TokenTree::Ident(i) => i == "Deserialize",
        proc_macro2::TokenTree::Group(g) => mentions_deserialize(g.stream()),
        _ => false,
    })
}

/// `attrs` when `keep`, none otherwise: the attributes the derive doesn't read go on the type with the item's own
/// name only (an owned mirror may not derive what they belong to).
pub(crate) fn maybe_attrs(keep: bool, attrs: &[syn::Attribute]) -> &[syn::Attribute] {
    if keep { attrs } else { &[] }
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

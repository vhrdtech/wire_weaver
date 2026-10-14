//! `#[common] Common(CommonError<'i>)` enum variant: `From<CommonError>` and `ww_error::HasCommon` (STD-16).

use crate::ast::item_enum::Fields;
use crate::codegen::item_enum::CGItemEnum;
use crate::codegen::util::maybe_quote;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;

/// How generated code names the `ww_error` crate: `::ww_error` if it is a dependency (also inside `ww_error`
/// itself, thanks to `extern crate self`), else through `wire_weaver`'s re-export.
fn ww_error_path() -> Result<TokenStream, String> {
    use proc_macro_crate::{FoundCrate, crate_name};
    match crate_name("ww_error") {
        Ok(FoundCrate::Itself) => Ok(quote! { ::ww_error }),
        Ok(FoundCrate::Name(name)) => {
            let name = Ident::new(&name, Span::call_site());
            Ok(quote! { ::#name })
        }
        Err(_) => match crate_name("wire_weaver") {
            Ok(FoundCrate::Itself) => Ok(quote! { crate::ww_error }),
            Ok(FoundCrate::Name(name)) => {
                let name = Ident::new(&name, Span::call_site());
                Ok(quote! { ::#name::ww_error })
            }
            Err(_) => Err(
                "#[common] needs `ww_error` or `wire_weaver` as a dependency, for `ww_error::HasCommon`".into(),
            ),
        },
    }
}

impl CGItemEnum<'_> {
    /// `From<CommonError>` and `HasCommon` for the `#[common]` variant, nothing without one.
    pub(crate) fn impl_common(&self, is_ref: bool) -> Result<TokenStream, String> {
        let Some(variant) = self.variants.iter().find(|v| v.common) else {
            return Ok(quote! {});
        };
        let Fields::Unnamed(fields) = &variant.fields else {
            return Ok(quote! {});
        };
        let ww_error = ww_error_path()?;
        let cfg = self.cfg.map(|cond| quote! { #[cfg(#cond)] });
        let variant_cfg = variant.cfg_attrs();
        let enum_name = self.ident;
        let variant_ident = &variant.ident;
        let common_ty = fields[0].def(is_ref, self.crate_path);
        let lifetime = maybe_quote(is_ref && !self.ambiguous, || quote! { <'i> });
        Ok(quote! {
            #cfg
            #variant_cfg
            impl #lifetime From<#common_ty> for #enum_name #lifetime {
                fn from(e: #common_ty) -> Self {
                    Self::#variant_ident(e)
                }
            }

            #cfg
            #variant_cfg
            impl #lifetime #ww_error::HasCommon for #enum_name #lifetime {
                type Common = #common_ty;

                // not `Self::Common`, it would be ambiguous with a variant named `Common`
                fn common(&self) -> Option<&#common_ty> {
                    #[allow(unreachable_patterns)]
                    match self {
                        Self::#variant_ident(e) => Some(e),
                        _ => None,
                    }
                }
            }
        })
    }
}

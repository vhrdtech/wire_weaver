use crate::codegen::CodegenBundle;
use crate::codegen::ty_def::{TyPos, ty_def};
use crate::codegen::util::maybe_quote;
use convert_case::Casing;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{TokenStreamExt, quote};
use ww_self::{ApiItemKindOwned, ApiLevelOwned};

/// Name of the struct a method's arguments are serialized through. With alloc the struct holds owned types
/// (`Vec`, `String`), and shrink_wrap wants such a struct's name to end in `Owned`.
pub fn args_struct_ident(method: &str, no_alloc: bool) -> Ident {
    let suffix = if no_alloc { "" } else { "Owned" };
    Ident::new(
        &format!(
            "{}{suffix}",
            format!("{method}_args").to_case(convert_case::Case::Pascal)
        ),
        Span::call_site(),
    )
}

pub fn args_structs(
    api_bundle: &CodegenBundle,
    api_level: &ApiLevelOwned,
    no_alloc: bool,
) -> TokenStream {
    let mut defs = TokenStream::new();
    for item in &api_level.items {
        let args = match &item.kind {
            ApiItemKindOwned::Method { args, .. } | ApiItemKindOwned::StreamFn { args, .. } => {
                Some(args)
            }
            _ => None,
        };
        if let Some(args) = args {
            if args.is_empty() {
                continue;
            }
            let ident = args_struct_ident(&item.ident, no_alloc);
            let is_lifetime = args
                .iter()
                .any(|arg| arg.ty.is_lifetime(api_bundle).unwrap());
            // `ty_def` only ever keeps a real borrow around when rendering the alloc-free
            // (`no_alloc`) fields; with alloc available every field is converted to its owned,
            // lifetime-free form (see `user_ty_def`), regardless of `is_lifetime`.
            let needs_lifetime = no_alloc && is_lifetime;
            let maybe_lifetime = maybe_quote(needs_lifetime, quote! { <'i> });
            if let [arg] = args.as_slice()
                && arg.is_flatten()
            {
                // EVO-22: the argument's struct is the args struct itself, its fields are the arguments
                let ty = ty_def(api_bundle, &arg.ty, !no_alloc, TyPos::Def).unwrap();
                defs.append_all(quote! {
                    pub(crate) type #ident #maybe_lifetime = #ty;
                });
                continue;
            }
            let fields = args.iter().map(|f| {
                let ident = Ident::new(&f.ident, Span::call_site());
                let ty = ty_def(api_bundle, &f.ty, !no_alloc, TyPos::Def).unwrap();
                // a client of a version without the argument sends fewer bytes (EVO-22)
                let default = f.default().map(|default| {
                    let default: syn::Expr = syn::parse_str(default).unwrap_or_else(|e| {
                        panic!("argument `{}`: default `{default}`: {e}", f.ident)
                    });
                    quote! { #[default = #default] }
                });
                quote! { #default #ident: #ty }
            });
            // When none of the args end up needing a lifetime (e.g. all-plain scalar args, or
            // alloc-based args that were converted to their owned form), the generated struct is
            // otherwise ambiguous to derive_shrink_wrap - disambiguate it towards whichever
            // representation this call site actually needs.
            let disambiguate = maybe_quote(
                !needs_lifetime,
                if no_alloc {
                    quote! { borrowed, }
                } else {
                    // The caller already committed to an alloc-capable target by asking for
                    // `no_alloc = false`, so this doesn't need its own "std" cfg gate.
                    quote! { owned, }
                },
            );
            defs.append_all(quote! {
                #[derive_shrink_wrap(#disambiguate)]
                struct #ident #maybe_lifetime {
                    #(#fields),*
                }
            });
        }
    }
    defs
}

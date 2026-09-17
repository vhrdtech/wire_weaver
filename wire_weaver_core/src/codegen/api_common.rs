use crate::codegen::ty_def::{TyPos, ty_def};
use crate::codegen::util::maybe_quote;
use convert_case::Casing;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{TokenStreamExt, quote};
use ww_self::{ApiBundleOwned, ApiItemKindOwned, ApiLevelOwned};

pub fn args_structs(
    api_bundle: &ApiBundleOwned,
    api_level: &ApiLevelOwned,
    no_alloc: bool,
) -> TokenStream {
    let mut defs = TokenStream::new();
    for item in &api_level.items {
        if let ApiItemKindOwned::Method { args, .. } = &item.kind {
            if args.is_empty() {
                continue;
            }
            let fields = args.iter().map(|f| {
                let ident = Ident::new(&f.ident, Span::call_site());
                let ty = ty_def(api_bundle, &f.ty, !no_alloc, TyPos::Def).unwrap();
                quote! { #ident: #ty }
            });

            let ident = Ident::new(
                format!("{}_args", &item.ident)
                    .to_case(convert_case::Case::Pascal)
                    .as_str(),
                Span::call_site(),
            );
            let is_lifetime = args
                .iter()
                .any(|arg| arg.ty.is_lifetime(api_bundle).unwrap());
            // `ty_def` only ever keeps a real borrow around when rendering the alloc-free
            // (`no_alloc`) fields; with alloc available every field is converted to its owned,
            // lifetime-free form (see `user_ty_def`), regardless of `is_lifetime`.
            let needs_lifetime = no_alloc && is_lifetime;
            let maybe_lifetime = maybe_quote(needs_lifetime, quote! { <'i> });
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

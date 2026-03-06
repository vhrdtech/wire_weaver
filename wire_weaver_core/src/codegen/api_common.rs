use crate::codegen::ty_def::ty_def;
use crate::codegen::util::maybe_quote;
use convert_case::Casing;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{quote, TokenStreamExt};
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
                let ty = ty_def(api_bundle, &f.ty, !no_alloc, false).unwrap();
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
                .any(|arg| arg.ty.is_unsized(api_bundle).unwrap());
            let maybe_lifetime = maybe_quote(is_lifetime, quote! { <'i> });
            defs.append_all(quote! {
                #[derive_shrink_wrap]
                struct #ident #maybe_lifetime {
                    #(#fields),*
                }
            });
        }
    }
    defs
}

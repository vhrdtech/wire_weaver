use crate::codegen::ty_def::ty_def;
use convert_case::Casing;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{quote, TokenStreamExt};
use ww_self::{ApiBundleOwned, ApiItemKindOwned, ApiLevelOwned};

pub fn args_structs(
    api_bundle: &ApiBundleOwned,
    api_level: &ApiLevelOwned,
    _no_alloc: bool,
) -> TokenStream {
    let mut defs = TokenStream::new();
    for item in &api_level.items {
        if let ApiItemKindOwned::Method { args, .. } = &item.kind {
            if args.is_empty() {
                continue;
            }
            let fields = args.iter().map(|f| {
                let ident = Ident::new(&f.ident, Span::call_site());
                let ty = ty_def(api_bundle, &f.ty, true, false).unwrap();
                quote! { #ident: #ty }
            });

            let ident = Ident::new(
                format!("{}_args", &item.ident)
                    .to_case(convert_case::Case::Pascal)
                    .as_str(),
                Span::call_site(),
            );
            defs.append_all(quote! {
                #[derive_shrink_wrap]
                struct #ident {
                    #(#fields),*
                }
            });
        }
    }
    defs
}

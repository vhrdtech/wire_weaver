use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::{Error, ItemTrait};

pub fn ww_trait(attr: TokenStream, item: TokenStream) -> TokenStream {
    ww_trait_inner(attr, item)
        .unwrap_or_else(|e| Error::new(Span::call_site(), e).to_compile_error())
}

fn ww_trait_inner(_attr: TokenStream, item: TokenStream) -> Result<TokenStream, Error> {
    let item_trait: ItemTrait = syn::parse2(item)?;
    let docs: Vec<_> = item_trait
        .attrs
        .iter()
        .filter(|a| a.path().is_ident("doc"))
        .collect();
    let ident = Ident::new(
        item_trait
            .ident
            .to_string()
            .to_case(Case::Constant)
            .as_str(),
        item_trait.ident.span(),
    );
    Ok(quote! {
        #(#docs)*
        pub const #ident: () = ();
    })
}

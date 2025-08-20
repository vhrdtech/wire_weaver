use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::{Error, ItemTrait};
use wire_weaver_core::ast::api::ApiLevelSourceLocation;
use wire_weaver_core::transform::transform_api_level::transform_api_level;

pub fn ww_trait(attr: TokenStream, item: TokenStream) -> TokenStream {
    ww_trait_inner(attr, item)
        .unwrap_or_else(|e| Error::new(Span::call_site(), e).to_compile_error())
}

fn ww_trait_inner(_attr: TokenStream, item: TokenStream) -> Result<TokenStream, Error> {
    let item_trait: ItemTrait = syn::parse2(item)?;
    let api_level = transform_api_level(
        &item_trait,
        ApiLevelSourceLocation::File {
            path: Default::default(),
            part_of_crate: Ident::new("dummy", Span::call_site()),
        },
    )
    .map_err(|e| Error::new(Span::call_site(), e))?;

    let mut check_types_lifetimes = TokenStream::new();
    for (ty, lifetime) in api_level.external_types() {
        let check_name = if lifetime {
            "ShouldHaveALifetime"
        } else {
            "ShouldNotHaveALifetime"
        };
        let Some(last) = ty.segments.last() else {
            continue;
        };
        let check_name = Ident::new(format!("_{last}{check_name}",).as_str(), last.span());
        let ty = &ty;
        if lifetime {
            check_types_lifetimes.extend(quote! {
                type #check_name<'i> = #ty<'i>;
            });
        } else {
            check_types_lifetimes.extend(quote! {
                type #check_name = #ty;
            });
        }
    }

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
        #check_types_lifetimes
    })
}

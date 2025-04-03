use proc_macro2::{TokenStream, TokenTree};
use quote::quote;
use syn::{Expr, ItemEnum, Lit, parse2};

pub fn ww_repr(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut attr = attr.into_iter();
    if let TokenTree::Ident(repr) = attr.next().unwrap() {
        if repr != "u4" {
            panic!("Only u4 repr is supported");
        }
    } else {
        panic!("Only u4 repr is supported");
    }
    let enum_def: ItemEnum = parse2(item).unwrap();
    let mut prev_discriminant = None;
    for (idx, variant) in enum_def.variants.iter().enumerate() {
        if let Some((_, discriminant)) = &variant.discriminant {
            let Expr::Lit(discriminant) = discriminant else {
                panic!("Only literal discriminants are supported");
            };
            let Lit::Int(discriminant) = &discriminant.lit else {
                panic!("Only integer discriminants are supported");
            };
            let discriminant = discriminant.base10_parse::<u128>().unwrap();
            if discriminant > 15 {
                panic!("Discriminants above 15 are forbidden for u4 type");
            }
            prev_discriminant = Some(discriminant);
        } else if let Some(prev_discriminant) = prev_discriminant {
            let discriminant = prev_discriminant + 1;
            if discriminant > 15 {
                panic!(
                    "Variant '{}' have discriminant value of {}, overflowing u4 type",
                    variant.ident, discriminant
                );
            }
        } else {
            prev_discriminant = Some(0);
        }

        if idx > 15 {
            panic!("Maximum 16 variants are allowed with u4 discriminant");
        }
    }

    quote! {
        #[repr(u8)]
        #enum_def
    }
}

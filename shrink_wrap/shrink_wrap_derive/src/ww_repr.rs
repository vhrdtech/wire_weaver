use proc_macro2::{Ident, Span, TokenStream, TokenTree};
use quote::quote;
use shrink_wrap_core::ast::Repr;
use syn::{Expr, ItemEnum, Lit, Meta, parse2};

pub fn ww_repr(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut attr = attr.into_iter();
    let ww_repr = if let TokenTree::Ident(repr) = attr.next().unwrap() {
        Repr::parse_str(repr.to_string().as_str()).unwrap()
    } else {
        panic!("Wrong ww_repr provided, expected bool/u1..u32 or unib32");
    };
    let enum_def: ItemEnum = parse2(item).unwrap();
    let mut current_discriminant = 0;
    let mut max_discriminant = 0;
    for variant in enum_def.variants.iter() {
        let discriminant = if let Some((_, discriminant)) = &variant.discriminant {
            let Expr::Lit(discriminant) = discriminant else {
                panic!("Only literal discriminants are supported");
            };
            let Lit::Int(discriminant) = &discriminant.lit else {
                panic!("Only integer discriminants are supported");
            };
            let discriminant = discriminant.base10_parse::<u32>().unwrap();
            current_discriminant = discriminant;
            discriminant
        } else {
            let discriminant = current_discriminant;
            current_discriminant += 1;
            discriminant
        };
        max_discriminant = max_discriminant.max(discriminant);
    }
    if max_discriminant > ww_repr.max_discriminant() {
        panic!("Maximum discriminant exceeded the {ww_repr:?}");
    }

    let base_ty = Ident::new(
        format!("u{}", ww_repr.std_bits()).as_str(),
        Span::call_site(),
    );
    let maybe_repr_attr = if let Some(repr_attr) = enum_def
        .attrs
        .iter()
        .find(|attr| attr.path().is_ident("repr"))
    {
        // if #[repr(...)] is provided, ensure it is the same that would have been generated
        if let Meta::List(list) = &repr_attr.meta {
            let Some(repr) = list.tokens.clone().into_iter().next() else {
                panic!("Expected #[repr(u8 / u16 or u32)] attribute");
            };
            let TokenTree::Ident(repr) = repr else {
                panic!("Expected #[repr(u8 / u16 or u32)] attribute");
            };
            let available_bits = match repr.to_string().as_str() {
                "u8" => 8,
                "u16" => 16,
                "u32" => 32,
                r => panic!("Only u8 / u16 and u32 is supported, got '{r}'"),
            };
            if ww_repr.required_bits() > available_bits {
                panic!(
                    "repr used is not big enough to hold all the enum variants, required is u{}",
                    ww_repr.required_bits()
                );
            }
        } else {
            panic!("Wrong repr provided, expected no #[repr(...)] or #[repr(u8 / u16 or u32)]");
        };
        quote! {}
    } else {
        quote! {
            #[repr(#base_ty)]
        }
    };

    let enum_name = enum_def.ident.clone();
    let lifetimes = enum_def.generics.params.clone();
    quote! {
        #maybe_repr_attr
        #enum_def
        impl <#lifetimes> #enum_name <#lifetimes> {
            pub fn discriminant(&self) -> #base_ty {
                unsafe { *<*const _>::from(self).cast::<#base_ty>() }
            }
        }
    }
}

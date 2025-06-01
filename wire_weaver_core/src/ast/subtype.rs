use crate::ast::Type;
use crate::ast::Type::{F32, F64, I8, I16, I32, I64, I128};
use crate::ast::ident::Ident;
use proc_macro2::TokenStream;
use quote::quote;
use std::ops::RangeInclusive;
use syn::LitInt;

pub struct Subtype {
    name: Ident,
    base_ty: Type,
    valid_range: Option<RangeInclusive<LitInt>>,
    valid_list: Vec<LitInt>,
}

impl Subtype {
    pub fn ts(&self) -> Result<TokenStream, &'static str> {
        let name = &self.name;
        let check_valid_range = self.valid_range_check()?;
        let base_ty = self.base_ty.def(false);
        Ok(quote! {
            pub struct #name(#base_ty);

            impl #name {
                pub fn new(value: #base_ty) -> Option<Self> {
                    #check_valid_range
                    Some(Self(value))
                }

                // min, max, zero, one
            }
        })
    }

    fn valid_range_check(&self) -> Result<TokenStream, &'static str> {
        if let Some(range) = &self.valid_range {
            Ok(quote! {})
        } else {
            Ok(quote! {})
        }
    }

    fn get_value(&self) -> Result<TokenStream, &'static str> {
        use Type::*;
        match self.base_ty {
            U8 | U16 | U32 | U64 | U128 | I8 | I16 | I32 | I64 | I128 | F32 | F64 => {
                Ok(quote! { value })
            }
            U4 => Ok(quote! { value }),
            UNib32 => Ok(quote! { value.0 }),
            ULeb32 | ULeb64 | ULeb128 | ILeb32 | ILeb64 | ILeb128 => Err("not yet supported"),
            Bool => Err("not supported"),
            // I4 => {}
            // String => {}
            // Array(_, _) => {}
            // Tuple(_) => {}
            // Vec(_) => {}
            // Unsized(_, _) => {}
            // Sized(_, _) => {}
            // Option(_, _) => {}
            // Result(_, _) => {}
            IsSome(_) | IsOk(_) => Err("not supported"),
            _ => unimplemented!(),
        }
    }
}

use crate::file::{SynConversionError, SynConversionWarning};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use std::process::id;

#[derive(Debug)]
pub enum Type {
    // Array,
    Bool,
    Discrete(TypeDiscrete),
    // VariableLength,
    Floating(TypeFloating),
    // Str,
    // Path,
}

#[derive(Debug)]
pub struct TypeDiscrete {
    pub is_signed: bool,
    pub bits: u16,
    // unit
    // bounds
}

#[derive(Debug)]
pub struct TypeFloating {
    pub bits: u16, // unit
                   // bounds
}

impl Type {
    pub(crate) fn from_syn(
        ty: syn::Type,
    ) -> Result<(Self, Vec<SynConversionWarning>), Vec<SynConversionError>> {
        match ty {
            syn::Type::Path(type_path) => {
                if type_path.path.segments.len() == 1 {
                    let path_segment = type_path.path.segments.first().unwrap();
                    let ident = path_segment.ident.to_string();
                    if ident.starts_with('f') {
                        let bits: u16 = ident.strip_prefix('f').unwrap().parse().unwrap();
                        Ok((Type::Floating(TypeFloating { bits }), vec![]))
                    } else if ident.starts_with('u') {
                        let bits: u16 = ident.strip_prefix('u').unwrap().parse().unwrap();
                        Ok((
                            Type::Discrete(TypeDiscrete {
                                is_signed: false,
                                bits,
                            }),
                            vec![],
                        ))
                    } else if ident.starts_with('i') {
                        let bits: u16 = ident.strip_prefix('i').unwrap().parse().unwrap();
                        Ok((
                            Type::Discrete(TypeDiscrete {
                                is_signed: true,
                                bits,
                            }),
                            vec![],
                        ))
                    } else if ident == "bool" {
                        Ok((Type::Bool, vec![]))
                    } else {
                        Err(vec![SynConversionError::UnknownType])
                    }
                } else {
                    Err(vec![SynConversionError::UnknownType])
                }
            }
            _ => Err(vec![SynConversionError::UnknownType]),
        }
    }

    pub fn to_tokens(&self) -> TokenStream {
        match self {
            Type::Bool => quote!(bool),
            Type::Discrete(ty_discrete) => {
                todo!()
            }
            Type::Floating(ty_floating) => {
                if ty_floating.bits == 32 || ty_floating.bits == 64 {
                    let ty = format!("f{}", ty_floating.bits);
                    let ty = Ident::new(ty.as_str(), Span::call_site());
                    quote!(#ty)
                } else {
                    unimplemented!()
                }
            }
        }
    }

    pub fn to_ser_fn_name(&self) -> Ident {
        match self {
            Type::Bool => Ident::new("ser_bool", Span::call_site()),
            Type::Discrete(ty_discrete) => {
                todo!()
            }
            Type::Floating(ty_floating) => {
                let ser_fn = format!("ser_f{}", ty_floating.bits);
                Ident::new(ser_fn.as_str(), Span::call_site())
            }
        }
    }
}

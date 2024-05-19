use crate::ast::file::{SynConversionError, SynConversionWarning};
use crate::ast::ident::Ident;
use crate::ast::path::Path;

#[derive(Debug)]
pub enum Type {
    // Array,
    Bool,
    Discrete(TypeDiscrete),
    // VariableLength,
    Floating(TypeFloating),
    String,
    Path(Path),
    // Option(Path),
    // Result(Path, Path),
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
                    } else if ident == "String" {
                        Ok((Type::String, vec![]))
                    } else {
                        Ok((Type::Path(Path::new_ident(Ident::new(ident))), vec![]))
                    }
                } else {
                    Err(vec![SynConversionError::UnknownType])
                }
            }
            _ => Err(vec![SynConversionError::UnknownType]),
        }
    }
}

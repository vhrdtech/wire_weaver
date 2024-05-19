use crate::ast::data::{Fields, Variant};
use crate::ast::file::{SynConversionError, SynConversionWarning};
use crate::ast::ident::Ident;
use crate::ast::ty::Type;
use crate::ast::value::Value;
use crate::ast::version::Version;
use syn::{Expr, Lit, Meta};

#[derive(Debug)]
pub enum Item {
    Enum(ItemEnum),
    Struct(ItemStruct),
}

#[derive(Debug)]
pub struct ItemStruct {
    // attrs
    // generics
    pub ident: Ident,
    pub fields: Vec<StructField>,
}

#[derive(Debug)]
pub struct StructField {
    // attrs
    pub id: u32,
    pub ident: Ident,
    pub ty: Type,
    pub since: Option<Version>,
    pub default: Option<Value>,
}

#[derive(Debug)]
pub struct ItemEnum {
    // attrs
    // generics
    pub ident: Ident,
    pub variants: Vec<Variant>,
}

impl Item {
    pub(crate) fn from_syn(
        item: syn::Item,
    ) -> Result<(Option<Self>, Vec<SynConversionWarning>), Vec<SynConversionError>> {
        match item {
            syn::Item::Struct(item_struct) => match ItemStruct::from_syn(item_struct) {
                Ok((item_struct, warnings)) => Ok((Some(Item::Struct(item_struct)), warnings)),
                Err(e) => Err(e),
            },
            syn::Item::Enum(item_enum) => match ItemEnum::from_syn(item_enum) {
                Ok((item_enum, warnings)) => Ok((Some(Item::Enum(item_enum)), warnings)),
                Err(e) => Err(e),
            },
            // syn::Item::Mod(item_mod) => {
            //
            // }
            // syn::Item::Use(item_use) => {
            //
            // }
            // syn::Item::Type(item_type) => {}
            // syn::Item::Const(item_const) => {}
            _ => Ok((None, vec![SynConversionWarning::UnknownFileItem])),
        }
    }
}

impl ItemStruct {
    fn from_syn(
        item_struct: syn::ItemStruct,
    ) -> Result<(Self, Vec<SynConversionWarning>), Vec<SynConversionError>> {
        let mut fields = vec![];
        let mut errors = vec![];
        let mut warnings = vec![];
        for (idx, mut field) in item_struct.fields.into_iter().enumerate() {
            let ty = match Type::from_syn(field.ty) {
                Ok((ty, w)) => {
                    warnings.extend(w);
                    ty
                }
                Err(e) => {
                    errors.extend(e);
                    continue;
                }
            };
            fields.push(StructField {
                id: take_id_attr(&mut field.attrs).unwrap_or(idx as u32),
                ident: field.ident.unwrap().into(),
                ty,
                since: take_since_attr(&mut field.attrs),
                default: take_default_attr(&mut field.attrs, &mut errors),
            });
            for a in field.attrs {
                warnings.push(SynConversionWarning::UnknownAttribute(format!(
                    "{:?}",
                    a.meta.path()
                )));
            }
        }
        if errors.is_empty() {
            Ok((
                ItemStruct {
                    ident: item_struct.ident.into(),
                    fields,
                },
                warnings,
            ))
        } else {
            Err(errors)
        }
    }

    pub fn contains_ref_types(&self) -> bool {
        for f in &self.fields {
            if f.ty.is_ref() {
                return true;
            }
        }
        false
    }
}

impl ItemEnum {
    fn from_syn(
        item_enum: syn::ItemEnum,
    ) -> Result<(Self, Vec<SynConversionWarning>), Vec<SynConversionError>> {
        let mut variants = vec![];
        let mut errors = vec![];
        let mut warnings = vec![];
        let mut latest_discriminant = 0;
        for mut variant in item_enum.variants {
            let discriminant =
                Self::get_discriminant(&mut errors, &mut latest_discriminant, &variant);
            variants.push(Variant {
                ident: variant.ident.into(),
                fields: Fields::Unit,
                discriminant,
                since: take_since_attr(&mut variant.attrs),
            });
            for a in variant.attrs {
                warnings.push(SynConversionWarning::UnknownAttribute(format!(
                    "{:?}",
                    a.meta.path()
                )));
            }
        }
        if errors.is_empty() {
            Ok((
                ItemEnum {
                    ident: item_enum.ident.into(),
                    variants,
                },
                warnings,
            ))
        } else {
            Err(errors)
        }
    }

    fn get_discriminant(
        errors: &mut Vec<SynConversionError>,
        latest_discriminant: &mut u32,
        variant: &syn::Variant,
    ) -> u32 {
        variant
            .discriminant
            .as_ref()
            .map(|(_, expr)| {
                if let Expr::Lit(lit) = expr {
                    if let Lit::Int(lit_int) = &lit.lit {
                        let d = lit_int.base10_parse().unwrap();
                        *latest_discriminant = d;
                        d
                    } else {
                        errors.push(SynConversionError::WrongDiscriminant);
                        u32::MAX
                    }
                } else {
                    errors.push(SynConversionError::WrongDiscriminant);
                    u32::MAX
                }
            })
            .unwrap_or_else(|| {
                *latest_discriminant += 1;
                *latest_discriminant
            })
    }
}

/// Take `#[id = integer]` attribute and return the number
fn take_id_attr(attrs: &mut Vec<syn::Attribute>) -> Option<u32> {
    None
}

/// Take `#[since = vX.Y]` attribute and return the Version
fn take_since_attr(attrs: &mut Vec<syn::Attribute>) -> Option<Version> {
    None
}

/// Take `#[default = lit]` attribute and return Value containing provided literal
fn take_default_attr(
    attrs: &mut Vec<syn::Attribute>,
    errors: &mut Vec<SynConversionError>,
) -> Option<Value> {
    let (attr_idx, _) = attrs
        .iter()
        .enumerate()
        .find(|(_, a)| a.path().is_ident("default"))?;
    let attr = attrs.remove(attr_idx);
    let Meta::NameValue(name_value) = attr.meta else {
        errors.push(SynConversionError::WrongDefaultAttr(
            "Expected default = lit".into(),
        ));
        return None;
    };
    let Expr::Lit(expr_lit) = name_value.value else {
        errors.push(SynConversionError::WrongDefaultAttr(
            "Expected default = lit".into(),
        ));
        return None;
    };
    match expr_lit.lit {
        Lit::Float(lit_float) => {
            // TODO: Handle f32 and f64 properly
            Some(Value::F32(lit_float.base10_parse().unwrap()))
        }
        u => {
            errors.push(SynConversionError::WrongDefaultAttr(format!(
                "Not supported lit: {u:?}"
            )));
            None
        } // Lit::Str(_) => {}
          // Lit::ByteStr(_) => {}
          // Lit::CStr(_) => {}
          // Lit::Byte(_) => {}
          // Lit::Char(_) => {}
          // Lit::Int(_) => {}
          // Lit::Bool(_) => {}
          // Lit::Verbatim(_) => {}
    }
}

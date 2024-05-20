use crate::ast::data::{Field, Fields, FieldsNamed, FieldsUnnamed, Variant};
use crate::ast::ident::Ident;
use crate::ast::syn_convert::{
    collect_unknown_attributes, take_final_attr, take_since_attr, SynConversionError,
    SynConversionWarning,
};
use crate::ast::ty::Type;
use syn::{Expr, Lit};

#[derive(Debug)]
pub enum Item {
    Enum(ItemEnum),
    Struct(ItemStruct),
}

#[derive(Debug)]
pub struct ItemStruct {
    // attrs
    // generics
    pub is_final: bool,
    pub ident: Ident,
    pub fields: Vec<Field>,
}

#[derive(Debug)]
pub struct ItemEnum {
    // attrs
    // generics
    pub is_final: bool,
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
        mut item_struct: syn::ItemStruct,
    ) -> Result<(Self, Vec<SynConversionWarning>), Vec<SynConversionError>> {
        let mut fields = vec![];
        let mut errors = vec![];
        let mut warnings = vec![];
        for (def_order_idx, field) in item_struct.fields.into_iter().enumerate() {
            match Field::from_syn(def_order_idx as u32, field) {
                Ok((field, w)) => {
                    fields.push(field);
                    warnings.extend(w);
                }
                Err(e) => {
                    errors.extend(e);
                    continue;
                }
            };
        }
        if errors.is_empty() {
            collect_unknown_attributes(&mut item_struct.attrs, &mut warnings);
            Ok((
                ItemStruct {
                    ident: item_struct.ident.into(),
                    is_final: take_final_attr(&mut item_struct.attrs).is_some(),
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
    pub fn contains_data_fields(&self) -> bool {
        for variant in &self.variants {
            match variant.fields {
                Fields::Named(_) => {
                    return true;
                }
                Fields::Unnamed(_) => {
                    return true;
                }
                Fields::Unit => {}
            }
        }
        false
    }

    pub fn is_discriminant_only(&self) -> bool {
        self.is_final && !self.contains_data_fields()
    }

    fn from_syn(
        mut item_enum: syn::ItemEnum,
    ) -> Result<(Self, Vec<SynConversionWarning>), Vec<SynConversionError>> {
        let mut variants = vec![];
        let mut errors = vec![];
        let mut warnings = vec![];
        let mut latest_discriminant = 0;
        for mut variant in item_enum.variants {
            let discriminant =
                Self::get_discriminant(&mut errors, &mut latest_discriminant, &variant);
            let fields = Self::fields(variant.fields, &mut warnings, &mut errors);
            variants.push(Variant {
                ident: variant.ident.into(),
                fields,
                discriminant,
                since: take_since_attr(&mut variant.attrs),
            });
            collect_unknown_attributes(&mut variant.attrs, &mut warnings);
        }
        if errors.is_empty() {
            let is_final = take_final_attr(&mut item_enum.attrs).is_some();
            collect_unknown_attributes(&mut item_enum.attrs, &mut warnings);
            Ok((
                ItemEnum {
                    ident: item_enum.ident.into(),
                    variants,
                    is_final,
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

    fn fields(
        fields: syn::Fields,
        warnings: &mut Vec<SynConversionWarning>,
        errors: &mut Vec<SynConversionError>,
    ) -> Fields {
        match fields {
            syn::Fields::Named(fields_named) => {
                let mut named = vec![];
                for (def_order_idx, field) in fields_named.named.into_iter().enumerate() {
                    match Field::from_syn(def_order_idx as u32, field) {
                        Ok((field, w)) => {
                            named.push(field);
                            warnings.extend(w);
                        }
                        Err(e) => {
                            errors.extend(e);
                            continue;
                        }
                    }
                }
                Fields::Named(FieldsNamed { named })
            }
            syn::Fields::Unnamed(fields_unnamed) => {
                let mut unnamed = vec![];
                for (def_order_idx, field) in fields_unnamed.unnamed.into_iter().enumerate() {
                    match Field::from_syn(def_order_idx as u32, field) {
                        Ok((field, w)) => {
                            unnamed.push(field);
                            warnings.extend(w);
                        }
                        Err(e) => {
                            errors.extend(e);
                            continue;
                        }
                    }
                }
                Fields::Unnamed(FieldsUnnamed { unnamed })
            }
            syn::Fields::Unit => Fields::Unit,
        }
    }
}

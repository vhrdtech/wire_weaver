use syn::{Expr, Lit};

use crate::ast::ident::Ident;
use crate::ast::{Field, Fields, Item, ItemEnum, ItemStruct, Source, Type, Variant};
use crate::transform::syn_util::{
    collect_unknown_attributes, take_default_attr, take_final_attr, take_id_attr, take_repr_attr,
    take_since_attr,
};
use crate::transform::{Messages, SynConversionError, SynFile};

/// Go through items in syn AST form and transform into own AST.
/// Everything should be resolved and computed before this pass.
pub(crate) struct CollectAndConvertPass<'i> {
    pub(crate) files: &'i [SynFile],
    pub(crate) messages: &'i mut Messages,
    pub(crate) source: Source,
    pub(crate) item: &'i syn::Item,
}

impl<'i> CollectAndConvertPass<'i> {
    pub(crate) fn transform(&mut self) -> Option<Item> {
        match self.item {
            // Item::Const(_) => {}
            syn::Item::Enum(item_enum) => {
                self.transform_item_enum(item_enum).map(|e| Item::Enum(e))
            }
            // Item::Fn(_) => {}
            // Item::Mod(_) => {}
            // Item::Static(_) => {}
            syn::Item::Struct(item_struct) => self
                .transform_item_struct(item_struct)
                .map(|s| Item::Struct(s)),
            // Item::Trait(_) => {}
            // Item::Type(_) => {}
            // Item::Use(_) => {}
            // Item::Verbatim(_) => {}
            _ => None,
        }
    }

    fn transform_item_enum(&mut self, item_enum: &syn::ItemEnum) -> Option<ItemEnum> {
        let mut variants = vec![];
        let mut next_discriminant = 0;
        let mut bail = false;
        for variant in &item_enum.variants {
            let discriminant = self.get_discriminant(&mut next_discriminant, &variant);
            match self.convert_fields(&variant.fields) {
                Some(fields) => {
                    let mut attrs = variant.attrs.clone();
                    let since = take_since_attr(&mut attrs);
                    collect_unknown_attributes(&mut attrs, &mut self.messages);
                    variants.push(Variant {
                        ident: (&variant.ident).into(),
                        fields,
                        discriminant,
                        since,
                    });
                }
                None => bail = true,
            }
        }
        let mut attrs = item_enum.attrs.clone();
        let repr = take_repr_attr(&mut attrs, &mut self.messages).unwrap_or_default();
        if bail {
            None
        } else {
            let is_final = take_final_attr(&mut attrs).is_some();
            collect_unknown_attributes(&mut attrs, &mut self.messages);
            Some(ItemEnum {
                ident: (&item_enum.ident).into(),
                repr,
                variants,
                is_final,
            })
        }
    }

    fn get_discriminant(&mut self, next_discriminant: &mut u32, variant: &syn::Variant) -> u32 {
        variant
            .discriminant
            .as_ref()
            .map(|(_, expr)| {
                if let Expr::Lit(lit) = expr {
                    if let Lit::Int(lit_int) = &lit.lit {
                        let d = lit_int.base10_parse().unwrap();
                        *next_discriminant = d + 1;
                        d
                    } else {
                        self.messages
                            .push_conversion_error(SynConversionError::WrongDiscriminant);
                        u32::MAX
                    }
                } else {
                    self.messages
                        .push_conversion_error(SynConversionError::WrongDiscriminant);
                    u32::MAX
                }
            })
            .unwrap_or_else(|| {
                let d = *next_discriminant;
                *next_discriminant += 1;
                d
            })
    }

    fn convert_fields(&mut self, fields: &syn::Fields) -> Option<Fields> {
        match fields {
            syn::Fields::Named(fields_named) => {
                let mut named = vec![];
                for (def_order_idx, field) in fields_named.named.iter().enumerate() {
                    let field = self.transform_field(def_order_idx as u32, field)?;
                    named.push(field)
                }
                Some(Fields::Named(named))
            }
            syn::Fields::Unnamed(_fields_unnamed) => {
                todo!()
                // let mut unnamed = vec![];
                // for (def_order_idx, field) in fields_unnamed.unnamed.iter().enumerate() {
                // }
                // Some(Fields::Unnamed(unnamed))
            }
            syn::Fields::Unit => Some(Fields::Unit),
        }
    }

    pub fn transform_item_struct(&mut self, item_struct: &syn::ItemStruct) -> Option<ItemStruct> {
        let mut fields = vec![];
        let mut bail = false;
        for (def_order_idx, field) in item_struct.fields.iter().enumerate() {
            match self.transform_field(def_order_idx as u32, field) {
                Some(field) => {
                    fields.push(field);
                }
                None => bail = true,
            }
        }
        if bail {
            None
        } else {
            let mut attrs = item_struct.attrs.clone();
            let is_final = take_final_attr(&mut attrs).is_some();
            collect_unknown_attributes(&mut attrs, &mut self.messages);
            Some(ItemStruct {
                ident: (&item_struct.ident).into(),
                is_final,
                fields,
            })
        }
    }

    fn transform_field(&mut self, def_order_idx: u32, field: &syn::Field) -> Option<Field> {
        let mut field = field.clone();
        let ty = self.transform_type(field.ty)?;
        let default = take_default_attr(&mut field.attrs, &mut self.messages);
        collect_unknown_attributes(&mut field.attrs, &mut self.messages);
        Some(Field {
            id: take_id_attr(&mut field.attrs).unwrap_or(def_order_idx),
            ident: field
                .ident
                .map(|i| i.into())
                .unwrap_or(Ident::new(format!("_{def_order_idx}"))),
            ty,
            since: take_since_attr(&mut field.attrs),
            default,
        })
    }

    fn transform_type(&mut self, ty: syn::Type) -> Option<Type> {
        match ty {
            syn::Type::Path(type_path) => {
                if type_path.path.segments.len() == 1 {
                    let path_segment = type_path.path.segments.first().unwrap();
                    let ident = path_segment.ident.to_string();
                    let ty = match ident.as_str() {
                        "u8" => Type::U8,
                        _ => {
                            // go through current file and find it else emit error
                            self.messages
                                .push_conversion_error(SynConversionError::UnknownType);
                            return None;
                        }
                    };
                    Some(ty)
                } else {
                    // go through files and find it
                    self.messages
                        .push_conversion_error(SynConversionError::UnknownType);
                    None
                }
            }
            syn::Type::Array(_type_array) => {
                unimplemented!()
            }
            _ => {
                self.messages
                    .push_conversion_error(SynConversionError::UnknownType);
                None
            }
        }
    }
}

use crate::ast::ItemEnum;
use crate::ast::item_enum::{Fields, Variant};
use crate::ast::util::CfgAttrDefmt;
use crate::transform::docs_util::add_notes;
use crate::transform::syn_util::{
    collect_docs_attrs, collect_unknown_attributes, take_defmt_attr, take_derive_attr,
    take_since_attr, take_size_assumption, take_ww_repr_attr,
};
use crate::transform::transform_struct::{change_is_ok_to_is_some, propagate_default_to_flags};
use crate::transform::util::{
    FieldPath, FieldPathRoot, check_flag_order, create_flags, transform_field,
};
use syn::{Expr, Lit};

impl ItemEnum {
    pub fn from_syn(item_enum: &syn::ItemEnum) -> Result<Self, String> {
        let mut variants = vec![];
        let mut current_discriminant: u32 = 0;
        let mut max_discriminant: u32 = 0;
        for variant in &item_enum.variants {
            let discriminant = match get_discriminant(variant)? {
                Some(discriminant) => {
                    current_discriminant = discriminant;
                    discriminant
                }
                None => {
                    let d = current_discriminant;
                    current_discriminant = current_discriminant.saturating_add(1);
                    d
                }
            };
            max_discriminant = max_discriminant.max(discriminant);
            let path = FieldPath::new(FieldPathRoot::EnumVariant(variant.ident.clone()));
            let fields = convert_fields(&variant.fields, &path)?;
            let mut attrs = variant.attrs.clone();
            let since = take_since_attr(&mut attrs)?;
            let docs = collect_docs_attrs(&mut attrs);
            collect_unknown_attributes(&mut attrs);
            variants.push(Variant {
                docs,
                ident: variant.ident.clone(),
                fields,
                discriminant,
                since,
            });
        }
        let mut attrs = item_enum.attrs.clone();
        let repr = take_ww_repr_attr(&mut attrs)?;
        if max_discriminant > repr.max_discriminant() {
            return Err("Enum discriminant is not large enough".into());
        }
        let size_assumption = take_size_assumption(&mut attrs);
        let mut docs = collect_docs_attrs(&mut attrs);
        add_notes(&mut docs, size_assumption, true);
        let derive = take_derive_attr(&mut attrs);
        let defmt = take_defmt_attr(&mut attrs)?.map(CfgAttrDefmt);
        collect_unknown_attributes(&mut attrs);
        Ok(ItemEnum {
            docs,
            derive,
            ident: item_enum.ident.clone(),
            repr,
            explicit_ww_repr: true,
            variants,
            size_assumption,
            cfg: None,
            defmt,
        })
    }
}

fn get_discriminant(variant: &syn::Variant) -> Result<Option<u32>, String> {
    variant
        .discriminant
        .as_ref()
        .map(|(_, expr)| {
            if let Expr::Lit(lit) = expr {
                if let Lit::Int(lit_int) = &lit.lit {
                    let d = lit_int.base10_parse().unwrap();
                    Ok(Some(d))
                } else {
                    Err("Wrong discriminant".into())
                }
            } else {
                Err("Wrong discriminant".into())
            }
        })
        .unwrap_or(Ok(None))
}

fn convert_fields(fields: &syn::Fields, path: &FieldPath) -> Result<Fields, String> {
    match fields {
        syn::Fields::Named(fields_named) => {
            let mut named = vec![];
            let mut explicit_flags = vec![];
            for (def_order_idx, field_syn) in fields_named.named.iter().enumerate() {
                let (field, is_explicit_flag) =
                    transform_field(def_order_idx as u32, field_syn, path)?;
                if is_explicit_flag {
                    explicit_flags.push(field_syn.ident.clone().unwrap());
                }
                named.push(field)
            }
            create_flags(&mut named, &explicit_flags);
            check_flag_order(&named)?;
            propagate_default_to_flags(&mut named)?;
            change_is_ok_to_is_some(&mut named);
            Ok(Fields::Named(named))
        }
        syn::Fields::Unnamed(fields_unnamed) => {
            let mut unnamed = vec![];
            for (def_order_idx, field) in fields_unnamed.unnamed.iter().enumerate() {
                let (field, _is_explicit_flag) =
                    transform_field(def_order_idx as u32, field, path)?;
                // TODO: Do unnamed fields have to have since, id, default, etc?
                // TODO: explicit flags in unnamed fields?
                unnamed.push(field.ty);
            }
            Ok(Fields::Unnamed(unnamed))
        }
        syn::Fields::Unit => Ok(Fields::Unit),
    }
}

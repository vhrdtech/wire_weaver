use crate::ast::util::CfgAttrDefmt;
use crate::ast::value::Value;
use crate::ast::{Field, ItemStruct, Type};
use crate::transform::docs_util::add_notes;
use crate::transform::syn_util::{
    collect_docs_attrs, collect_unknown_attributes, take_defmt_attr, take_derive_attr,
    take_size_assumption,
};
use crate::transform::util::{
    FieldPath, FieldPathRoot, check_flag_order, create_flags, transform_field,
};

impl ItemStruct {
    pub fn from_syn(item_struct: &syn::ItemStruct) -> Result<Self, String> {
        let mut fields = vec![];
        let mut explicit_flags = vec![];
        for (def_order_idx, field_syn) in item_struct.fields.iter().enumerate() {
            let path = FieldPath::new(FieldPathRoot::NamedField(field_syn.ident.clone().unwrap()));
            let (field, is_explicit_flag) =
                transform_field(def_order_idx as u32, field_syn, &path)?;
            if is_explicit_flag {
                explicit_flags.push(field_syn.ident.clone().unwrap());
            }
            fields.push(field);
        }
        let mut attrs = item_struct.attrs.clone();
        let size_assumption = take_size_assumption(&mut attrs);
        let mut docs = collect_docs_attrs(&mut attrs);
        add_notes(&mut docs, size_assumption, false);
        let derive = take_derive_attr(&mut attrs);
        let defmt = take_defmt_attr(&mut attrs)?.map(CfgAttrDefmt);
        collect_unknown_attributes(&mut attrs);
        create_flags(&mut fields, &explicit_flags);
        check_flag_order(&fields)?;
        propagate_default_to_flags(&mut fields)?;
        change_is_ok_to_is_some(&mut fields);
        Ok(ItemStruct {
            docs,
            derive,
            ident: item_struct.ident.clone(),
            size_assumption,
            fields,
            cfg: None,
            defmt,
        })
    }
}

pub fn propagate_default_to_flags(fields: &mut [Field]) -> Result<(), String> {
    let mut set_to_default_false = vec![];
    let mut default_found = false;
    let mut default_is_not_last = false;
    for f in fields.iter() {
        if f.default.is_none() {
            if default_found {
                default_is_not_last = true;
            }
            continue;
        }
        default_found = true;
        let Some(default) = &f.default else { continue };
        if !matches!(f.ty, Type::Option(_, _)) {
            return Err("#[default = ...] used on a type that is not Option<T>".into());
        }
        if default != &Value::None {
            return Err("Unsupported default literal".into());
        }
        set_to_default_false.push(f.ident.clone());
    }
    for ident in set_to_default_false {
        for f in fields.iter_mut() {
            if let Type::IsSome(flag_for_ident) = &f.ty {
                if flag_for_ident != &ident {
                    continue;
                }
                f.default = Some(Value::Bool(false)); // read is_some flag as false on EOB
            } else if matches!(f.ty, Type::Option(_, _)) && f.ident == ident {
                f.default = None; // TODO: Change to actual default value
            }
        }
    }
    if default_is_not_last {
        return Err("Wrong evolved type order (default is not last)".into());
    }
    Ok(())
}

/// Change IsOk to IsSome for explicit flags, as a full field list is needed to determine which one to use.
pub fn change_is_ok_to_is_some(fields: &mut [Field]) {
    let mut flip = vec![];
    for (idx, f) in fields.iter().enumerate() {
        let Type::IsOk(ident) = &f.ty else { continue };
        if fields
            .iter()
            .any(|f| (f.ident == *ident) && matches!(f.ty, Type::Option(_, _)))
        {
            flip.push(idx);
        }
    }
    for (idx, f) in fields.iter_mut().enumerate() {
        if flip.contains(&idx) {
            let Type::IsOk(ident) = &f.ty else { continue };
            f.ty = Type::IsSome(ident.clone());
        }
    }
}

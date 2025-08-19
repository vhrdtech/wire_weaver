mod docs_util;
pub mod syn_util;
pub mod transform_api_level;
pub mod transform_enum;
pub mod transform_struct;
pub mod transform_ty;
// TODO: check that no fields and no variants have the same name

use crate::ast::{Docs, Field, Type};
use crate::transform::syn_util::{
    collect_docs_attrs, collect_unknown_attributes, take_default_attr, take_flag_attr,
    take_id_attr, take_since_attr,
};
use crate::transform::transform_ty::transform_type;
use proc_macro2::{Ident, Span};

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum FieldPathRoot {
    NamedField(Ident),
    EnumVariant(Ident),
    Argument(Ident),
    Output,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum FieldSelector {
    NamedField(Ident),
    Tuple(u32),
    Array(usize),
    ResultIfOk,
    ResultIfErr,
    OptionIsSome,
}

#[derive(Debug)]
pub struct FieldPath {
    root: FieldPathRoot,
    selectors: Vec<FieldSelector>,
}

impl FieldPath {
    pub fn new(root: FieldPathRoot) -> Self {
        FieldPath {
            root,
            selectors: vec![],
        }
    }

    #[allow(dead_code)]
    pub fn push(&mut self, selector: FieldSelector) {
        self.selectors.push(selector);
    }

    pub fn clone_and_push(&self, selector: FieldSelector) -> Self {
        FieldPath {
            root: self.root.clone(),
            selectors: self.selectors.iter().cloned().chain([selector]).collect(),
        }
    }

    pub fn flag_ident(&self) -> Ident {
        match &self.root {
            FieldPathRoot::NamedField(ident) | FieldPathRoot::Argument(ident) => {
                let ident = ident.to_string();
                Ident::new(format!("_{ident}_flag").as_str(), Span::call_site())
            }
            FieldPathRoot::EnumVariant(_enum_variant_name) => {
                if let Some(FieldSelector::NamedField(ident)) = self.selectors.first() {
                    let ident = ident.to_string();
                    Ident::new(format!("_{ident}_flag").as_str(), Span::call_site())
                } else {
                    Ident::new("_todo_flag", Span::call_site())
                }
            }
            FieldPathRoot::Output => Ident::new("_output_flag", Span::call_site()),
        }
    }
}

/// Create flags for Result or Option fields without explicitly defined ones.
pub fn create_flags(fields: &mut Vec<Field>, explicit_flags: &[Ident]) {
    let mut fields_without_flags = vec![];
    for (idx, f) in fields.iter().enumerate() {
        let is_flag_ty = matches!(f.ty, Type::Result(_, _) | Type::Option(_, _));
        if is_flag_ty && !explicit_flags.iter().any(|i| i == &f.ident) {
            fields_without_flags.push((idx, matches!(f.ty, Type::Result(_, _)), f.ident.clone()));
        }
    }
    for (shift, (pos, is_result, ident)) in fields_without_flags.into_iter().enumerate() {
        let flag_ident = Ident::new(format!("_{}_flag", ident).as_str(), ident.span());
        let flag = Field {
            docs: Docs::empty(),
            id: 0, // TODO: Adjust auto created flag IDs
            ident: flag_ident,
            ty: if is_result {
                Type::IsOk(ident)
            } else {
                Type::IsSome(ident)
            },
            since: None,
            default: None,
        };
        fields.insert(pos + shift, flag);
    }
}

/// Check that using stack for flags will work
pub fn check_flag_order(fields: &[Field]) -> Result<(), String> {
    let mut flags_stack = vec![];
    for field in fields.iter() {
        match &field.ty {
            Type::Result(_, _) | Type::Option(_, _) => {
                let Some(flag_ident) = flags_stack.pop() else {
                    return Err(format!(
                        "incorrect flag order (no flag), expected LIFO: {}",
                        field.ident
                    ));
                };
                if flag_ident != field.ident {
                    return Err(format!(
                        "incorrect flag order, expected LIFO: {}",
                        field.ident
                    ));
                }
            }
            Type::IsOk(ident) | Type::IsSome(ident) => {
                flags_stack.push(ident.clone());
            }
            _ => {}
        }
    }
    Ok(())
}

// fn transform_const(item_const: &syn::ItemConst) -> Result<ItemConst, String> {
//     let ty = transform_type(
//         item_const.ty.deref().clone(),
//         None,
//         &FieldPath::new(FieldPathRoot::Argument(item_const.ident.clone())),
//     )?;
//     let mut attrs = item_const.attrs.clone();
//     let docs = collect_docs_attrs(&mut attrs);
//     collect_unknown_attributes(&mut attrs);
//     Ok(ItemConst {
//         docs,
//         ident: item_const.ident.clone().into(),
//         ty,
//         value: item_const.expr.deref().clone(),
//     })
// }

pub fn transform_field(
    def_order_idx: u32,
    field: &syn::Field,
    path: &FieldPath,
) -> Result<(Field, bool), String> {
    let mut field = field.clone();
    let ident = field.ident.clone().unwrap_or(Ident::new(
        format!("_{def_order_idx}").as_str(),
        Span::call_site(),
    ));

    let path = if let Some(ident) = field.ident {
        path.clone_and_push(FieldSelector::NamedField(ident))
    } else {
        path.clone_and_push(FieldSelector::Tuple(def_order_idx))
    };

    let ty = transform_type(field.ty, Some(&mut field.attrs), &path)?;
    let default = take_default_attr(&mut field.attrs)?;
    let flag = take_flag_attr(&mut field.attrs);
    let id = take_id_attr(&mut field.attrs).unwrap_or(def_order_idx);
    let docs = collect_docs_attrs(&mut field.attrs);
    collect_unknown_attributes(&mut field.attrs);

    if flag.is_some() {
        if !matches!(ty, Type::Bool) {
            return Err(format!("{ident:?} flag type is not bool"));
        }
        let result_ident = ident;
        let ident = if flag.is_some() {
            Ident::new(
                format!("_{}_flag", result_ident).as_str(),
                result_ident.span(),
            )
        } else {
            result_ident.clone()
        };

        Ok((
            Field {
                docs,
                id,
                ident,
                ty: Type::IsOk(result_ident),
                since: None,
                default,
            },
            true,
        ))
    } else {
        Ok((
            Field {
                docs,
                id,
                ident,
                ty,
                since: take_since_attr(&mut field.attrs),
                default,
            },
            false,
        ))
    }
}

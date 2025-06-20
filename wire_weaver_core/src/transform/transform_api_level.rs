use super::syn_util::{collect_docs_attrs, collect_unknown_attributes, take_id_attr};
use super::transform_ty::{transform_return_type, transform_type};
use super::{FieldPath, FieldPathRoot};
use crate::ast::api::{
    ApiItem, ApiItemKind, ApiLevel, ApiLevelSourceLocation, Argument, Multiplicity,
};
use crate::ast::trait_macro_args::{ImplTraitMacroArgs, StreamMacroArgs};
use std::ops::Deref;
use syn::{FnArg, GenericParam, Pat, TraitItem};

pub fn transform_api_level(
    item_trait: &syn::ItemTrait,
    source_location: ApiLevelSourceLocation,
) -> Result<ApiLevel, String> {
    let mut items = vec![];
    let mut next_id = 0;
    for trait_item in item_trait.items.iter() {
        match trait_item {
            TraitItem::Const(_) => {}
            TraitItem::Fn(trait_item_fn) => {
                let mut args = vec![];
                for input in trait_item_fn.sig.inputs.iter() {
                    let FnArg::Typed(pat_type) = input else {
                        continue;
                    };
                    let Pat::Ident(arg_ident) = pat_type.pat.deref() else {
                        continue;
                    };
                    let ty = transform_type(
                        pat_type.ty.deref().clone(),
                        None,
                        &FieldPath::new(FieldPathRoot::Argument(arg_ident.ident.clone())),
                    )?;
                    args.push(Argument {
                        ident: arg_ident.ident.clone(),
                        ty,
                    })
                }
                let mut attrs = trait_item_fn.attrs.clone();
                let id = match take_id_attr(&mut attrs) {
                    Some(id) => {
                        next_id = id + 1;
                        id
                    }
                    None => {
                        let id = next_id;
                        next_id += 1;
                        id
                    }
                };
                let docs = collect_docs_attrs(&mut attrs);
                collect_unknown_attributes(&mut attrs);
                let multiplicity = if trait_item_fn.sig.generics.params.is_empty() {
                    Multiplicity::Flat
                } else {
                    const ERR: &str = "Only '<N: u32>' generic parameter is supported on methods";
                    if trait_item_fn.sig.generics.params.len() != 1 {
                        return Err(ERR.into());
                    }
                    let param = trait_item_fn.sig.generics.params.iter().next().unwrap();
                    let GenericParam::Type(type_param) = param else {
                        return Err(ERR.into());
                    };
                    if type_param.ident != "N" {
                        return Err(ERR.into());
                    }
                    if type_param.bounds.len() != 1 {
                        return Err(ERR.into());
                    }
                    // let bound = type_param.bounds.iter().next().unwrap();

                    Multiplicity::Array { size_bound: 0 }
                };
                items.push(ApiItem {
                    id,
                    docs,
                    multiplicity,
                    kind: ApiItemKind::Method {
                        ident: trait_item_fn.sig.ident.clone(),
                        args,
                        return_type: transform_return_type(
                            trait_item_fn.sig.output.clone(),
                            &FieldPath::new(FieldPathRoot::Output),
                        )?,
                    },
                });
            }
            TraitItem::Type(_) => {}
            TraitItem::Macro(item_macro) => {
                let mut attrs = item_macro.attrs.clone();
                let id = match take_id_attr(&mut attrs) {
                    Some(id) => {
                        next_id = id + 1;
                        id
                    }
                    None => {
                        let id = next_id;
                        next_id += 1;
                        id
                    }
                };
                let kind = item_macro.mac.path.get_ident().unwrap().to_string();
                let docs = collect_docs_attrs(&mut attrs);
                // stream and sink from the server perspective
                if kind == "stream" || kind == "sink" {
                    let stream_args: StreamMacroArgs =
                        syn::parse2(item_macro.mac.tokens.clone()).unwrap();
                    let path = FieldPath::new(FieldPathRoot::NamedField(stream_args.ident.clone())); // TODO: Clarify FieldPath purpose
                    let is_up = kind == "stream";
                    items.push(ApiItem {
                        id,
                        docs,
                        multiplicity: stream_args.resource_array.multiplicity,
                        kind: ApiItemKind::Stream {
                            ident: stream_args.ident,
                            // ty: Type::Unsized(Path::new_ident(stream_args.ty_name.into()), false),
                            ty: transform_type(stream_args.ty, None, &path)?,
                            is_up,
                        },
                    });
                } else if kind == "property" {
                    let stream_args: StreamMacroArgs =
                        syn::parse2(item_macro.mac.tokens.clone()).unwrap();
                    let path = FieldPath::new(FieldPathRoot::NamedField(stream_args.ident.clone())); // TODO: Clarify FieldPath purpose
                    items.push(ApiItem {
                        id,
                        docs,
                        multiplicity: stream_args.resource_array.multiplicity,
                        kind: ApiItemKind::Property {
                            ident: stream_args.ident,
                            ty: transform_type(stream_args.ty, None, &path)?,
                        },
                    });
                } else if kind == "ww_impl" {
                    let args: ImplTraitMacroArgs =
                        syn::parse2(item_macro.mac.tokens.clone()).unwrap();
                    items.push(ApiItem {
                        id,
                        docs,
                        multiplicity: args.resource_array.multiplicity,
                        kind: ApiItemKind::ImplTrait { args, level: None },
                    });
                } else {
                    return Err(format!("Unknown API resource {kind}"));
                }
                collect_unknown_attributes(&mut attrs);
            }
            TraitItem::Verbatim(_) => {}
            _ => {}
        }
    }
    let mut attrs = item_trait.attrs.clone();
    let docs = collect_docs_attrs(&mut attrs);
    Ok(ApiLevel {
        docs,
        name: item_trait.ident.clone(),
        items,
        source_location,
    })
}

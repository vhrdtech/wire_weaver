use proc_macro2::Span;
use syn::{Expr, GenericArgument, Lit, PathArguments, PathSegment};

use crate::ast::ident::Ident;
use crate::ast::{Field, Fields, Item, ItemEnum, ItemStruct, Layout, Source, Type, Variant};
use crate::transform::syn_util::{
    collect_unknown_attributes, take_default_attr, take_final_attr, take_flag_attr, take_id_attr,
    take_repr_attr, take_since_attr,
};
use crate::transform::{Messages, SynConversionError, SynFile, SynItemWithContext};

/// Go through items in syn AST form and transform into own AST.
/// Everything should be resolved and computed before this pass.
pub(crate) struct CollectAndConvertPass<'i> {
    pub(crate) _files: &'i [SynFile],
    pub(crate) messages: &'i mut Messages,
    pub(crate) _source: Source,
    pub(crate) item: &'i SynItemWithContext,
}

#[derive(Clone)]
pub enum FieldPathRoot {
    NamedField(syn::Ident),
    EnumVariant(syn::Ident),
}

#[derive(Clone)]
pub enum FieldSelector {
    NamedField(syn::Ident),
    Tuple(u32),
    Array(usize),
    ResultIfOk,
    ResultIfErr,
    OptionIsSome,
}

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

    pub fn push(&mut self, selector: FieldSelector) {
        self.selectors.push(selector);
    }

    pub fn clone_and_push(&self, selector: FieldSelector) -> Self {
        FieldPath {
            root: self.root.clone(),
            selectors: self.selectors.iter().cloned().chain([selector]).collect(),
        }
    }

    pub fn flag_ident(&self) -> syn::Ident {
        match &self.root {
            FieldPathRoot::NamedField(ident) => {
                let ident = ident.to_string();
                syn::Ident::new(format!("_{ident}_flag").as_str(), Span::call_site())
            }
            FieldPathRoot::EnumVariant(_) => unimplemented!(),
        }
    }
}

impl<'i> CollectAndConvertPass<'i> {
    pub(crate) fn transform(&mut self) -> Option<Item> {
        match self.item {
            // Item::Const(_) => {}
            SynItemWithContext::Enum { item_enum } => {
                self.transform_item_enum(item_enum).map(Item::Enum)
            }
            // Item::Fn(_) => {}
            // Item::Mod(_) => {}
            // Item::Static(_) => {}
            SynItemWithContext::Struct { item_struct } => {
                self.transform_item_struct(item_struct).map(Item::Struct)
            } // Item::Trait(_) => {}
              // Item::Type(_) => {}
              // Item::Use(_) => {}
              // Item::Verbatim(_) => {}
              // _ => None,
        }
    }

    fn transform_item_enum(&mut self, item_enum: &syn::ItemEnum) -> Option<ItemEnum> {
        let mut variants = vec![];
        let mut next_discriminant = 0;
        let mut bail = false;
        for variant in &item_enum.variants {
            let discriminant = self.get_discriminant(&mut next_discriminant, variant);
            let path = FieldPath::new(FieldPathRoot::EnumVariant(variant.ident.clone()));
            match self.convert_fields(&variant.fields, &path) {
                Some(fields) => {
                    let mut attrs = variant.attrs.clone();
                    let since = take_since_attr(&mut attrs);
                    collect_unknown_attributes(&mut attrs, self.messages);
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
        let repr = take_repr_attr(&mut attrs, self.messages).unwrap_or_default();
        if bail {
            None
        } else {
            let is_final = take_final_attr(&mut attrs).is_some();
            collect_unknown_attributes(&mut attrs, self.messages);
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

    fn convert_fields(&mut self, fields: &syn::Fields, path: &FieldPath) -> Option<Fields> {
        match fields {
            syn::Fields::Named(fields_named) => {
                let mut named = vec![];
                for (def_order_idx, field) in fields_named.named.iter().enumerate() {
                    let field = self.transform_field(def_order_idx as u32, field, path)?;
                    named.push(field)
                }
                Some(Fields::Named(named))
            }
            syn::Fields::Unnamed(fields_unnamed) => {
                let mut unnamed = vec![];
                for (def_order_idx, field) in fields_unnamed.unnamed.iter().enumerate() {
                    let field = self.transform_field(def_order_idx as u32, field, path)?;
                    // TODO: Do unnamed fields have to have since, id, default, etc?
                    unnamed.push(field.ty);
                }
                Some(Fields::Unnamed(unnamed))
            }
            syn::Fields::Unit => Some(Fields::Unit),
        }
    }

    pub fn transform_item_struct(&mut self, item_struct: &syn::ItemStruct) -> Option<ItemStruct> {
        let mut fields = vec![];
        let mut bail = false;
        for (def_order_idx, field) in item_struct.fields.iter().enumerate() {
            let path = FieldPath::new(FieldPathRoot::NamedField(field.ident.clone().unwrap()));
            match self.transform_field(def_order_idx as u32, field, &path) {
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
            collect_unknown_attributes(&mut attrs, self.messages);
            Some(ItemStruct {
                ident: (&item_struct.ident).into(),
                is_final,
                fields,
            })
        }
    }

    fn transform_field(
        &mut self,
        def_order_idx: u32,
        field: &syn::Field,
        path: &FieldPath,
    ) -> Option<Field> {
        let mut field = field.clone();
        let ident = field
            .ident
            .clone()
            .map(|i| i.into())
            .unwrap_or(Ident::new(format!("_{def_order_idx}")));

        let path = if let Some(ident) = field.ident {
            path.clone_and_push(FieldSelector::NamedField(ident))
        } else {
            path.clone_and_push(FieldSelector::Tuple(def_order_idx))
        };

        let ty = self.transform_type(field.ty, &path)?;
        let default = take_default_attr(&mut field.attrs, self.messages);
        let flag = take_flag_attr(&mut field.attrs);
        collect_unknown_attributes(&mut field.attrs, self.messages);
        let id = take_id_attr(&mut field.attrs).unwrap_or(def_order_idx);

        if flag.is_some() {
            if !matches!(ty, Type::Bool) {
                self.messages
                    .push_conversion_error(SynConversionError::FlagTypeIsNotBool);
                return None;
            }
            let result_ident = ident;
            let ident = if flag.is_some() {
                Ident::new(format!("_{}_flag", result_ident.sym))
            } else {
                result_ident.clone()
            };

            Some(Field {
                id,
                ident,
                ty: Type::IsOk(result_ident),
                since: None,
                default,
            })
        } else {
            Some(Field {
                id,
                ident,
                ty,
                since: take_since_attr(&mut field.attrs),
                default,
            })
        }
    }

    fn transform_type(&mut self, ty: syn::Type, path: &FieldPath) -> Option<Type> {
        match ty {
            syn::Type::Path(type_path) => {
                if type_path.path.segments.len() == 1 {
                    let path_segment = type_path.path.segments.first().unwrap();
                    let ident = path_segment.ident.to_string();
                    let ty = match ident.as_str() {
                        "bool" => Type::Bool,
                        "u8" => Type::U8,
                        "u16" => Type::U16,
                        "u32" => Type::U32,
                        "u64" => Type::U64,
                        "u128" => Type::U128,
                        "nib16" => Type::Nib16,
                        "uleb32" => Type::ULeb32,
                        "uleb64" => Type::ULeb64,
                        "uleb128" => Type::ULeb128,
                        "i8" => Type::I8,
                        "i16" => Type::I16,
                        "i32" => Type::I32,
                        "i64" => Type::I64,
                        "i128" => Type::I128,
                        "ileb32" => Type::ILeb32,
                        "ileb64" => Type::ILeb64,
                        "ileb128" => Type::ILeb128,
                        "f32" => Type::F32,
                        "f64" => Type::F64,
                        "String" => Type::String,
                        "Vec" => return self.transform_type_vec(path_segment, path),
                        "Result" => return self.transform_type_result(path_segment, path),
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

    fn transform_type_result(
        &mut self,
        path_segment: &PathSegment,
        path: &FieldPath,
    ) -> Option<Type> {
        let PathArguments::AngleBracketed(arg) = &path_segment.arguments else {
            self.messages
                .push_conversion_error(SynConversionError::UnsupportedType(
                    "expected Result<T, E>, got Result or Result()".into(),
                ));
            return None;
        };
        let mut args = arg.args.iter();
        let (Some(ok_arg), Some(err_arg)) = (args.next(), args.next()) else {
            self.messages
                .push_conversion_error(SynConversionError::UnsupportedType(
                    "expected Result<T, E>".into(),
                ));
            return None;
        };
        let (GenericArgument::Type(ok_ty), GenericArgument::Type(err_ty)) = (ok_arg, err_arg)
        else {
            self.messages
                .push_conversion_error(SynConversionError::UnsupportedType(format!(
                    "expected Vec<T>, got {arg:?}"
                )));
            return None;
        };
        let ok_path = path.clone_and_push(FieldSelector::ResultIfOk);
        let ok_ty = self.transform_type(ok_ty.clone(), &ok_path)?;
        let err_path = path.clone_and_push(FieldSelector::ResultIfErr);
        let err_ty = self.transform_type(err_ty.clone(), &err_path)?;
        let flag_ident = path.flag_ident().into();
        Some(Type::Result(flag_ident, Box::new((ok_ty, err_ty))))
    }

    fn transform_type_vec(&mut self, path_segment: &PathSegment, path: &FieldPath) -> Option<Type> {
        let PathArguments::AngleBracketed(arg) = &path_segment.arguments else {
            self.messages
                .push_conversion_error(SynConversionError::UnsupportedType(
                    "expected Vec<T>, got Vec or Vec()".into(),
                ));
            return None;
        };
        let Some(arg) = arg.args.first() else {
            self.messages
                .push_conversion_error(SynConversionError::UnsupportedType(
                    "expected Vec<T>, got Vec<T, ?>".into(),
                ));
            return None;
        };
        let GenericArgument::Type(inner_ty) = arg else {
            self.messages
                .push_conversion_error(SynConversionError::UnsupportedType(format!(
                    "expected Vec<T>, got {arg:?}"
                )));
            return None;
        };
        let inner_ty = self.transform_type(inner_ty.clone(), path)?;
        Some(Type::Vec(Layout::Builtin(Box::new(inner_ty))))
    }
}

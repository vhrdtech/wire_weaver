use std::ops::Deref;

use crate::ast::api::{ApiItem, ApiItemKind, ApiLevel, Argument, Multiplicity};
use crate::ast::ident::Ident;
use crate::ast::path::Path;
use crate::ast::value::Value;
use crate::ast::{
    Docs, Field, Fields, ItemConst, ItemEnum, ItemStruct, Layout, Source, Type, Variant,
};
use crate::transform::docs_util::add_notes;
use crate::transform::syn_util::{
    collect_docs_attrs, collect_unknown_attributes, take_default_attr, take_derive_attr,
    take_flag_attr, take_id_attr, take_since_attr, take_size_assumption, take_ww_repr_attr,
};
use crate::transform::{
    ItemEnumTransformed, ItemStructTransformed, Message, Messages, SynConversionError, SynFile,
    SynItemWithContext,
};
use proc_macro2::Span;
use shrink_wrap::ElementSize;
use syn::parse::{Parse, ParseStream};
use syn::{
    Attribute, Expr, FnArg, GenericArgument, Lit, Pat, PathArguments, PathSegment, ReturnType,
    TraitItem,
};

/// Go through items in syn AST form and transform into own AST.
/// Everything should be resolved and computed before this pass.
pub(crate) struct CollectAndConvertPass<'i> {
    pub(crate) _files: &'i [SynFile],
    pub(crate) current_file: &'i SynFile,
    pub(crate) messages: &'i mut Messages,
    pub(crate) _source: Source,
    // pub(crate) is_shrink_wrap_attr_macro: bool,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum FieldPathRoot {
    NamedField(syn::Ident),
    EnumVariant(syn::Ident),
    Argument(syn::Ident),
    Output,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum FieldSelector {
    NamedField(syn::Ident),
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

    pub fn flag_ident(&self) -> syn::Ident {
        match &self.root {
            FieldPathRoot::NamedField(ident) | FieldPathRoot::Argument(ident) => {
                let ident = ident.to_string();
                syn::Ident::new(format!("_{ident}_flag").as_str(), Span::call_site())
            }
            FieldPathRoot::EnumVariant(_enum_variant_name) => {
                if let Some(selector) = self.selectors.first() {
                    if let FieldSelector::NamedField(ident) = selector {
                        let ident = ident.to_string();
                        syn::Ident::new(format!("_{ident}_flag").as_str(), Span::call_site())
                    } else {
                        syn::Ident::new("_todo_flag", Span::call_site())
                    }
                } else {
                    syn::Ident::new("_todo_flag", Span::call_site())
                }
            }
            FieldPathRoot::Output => syn::Ident::new("_output_flag", Span::call_site()),
        }
    }
}

impl CollectAndConvertPass<'_> {
    pub(crate) fn transform(&mut self, item: &mut SynItemWithContext) {
        match item {
            // Item::Const(_) => {}
            SynItemWithContext::Enum {
                item_enum,
                transformed,
            } => {
                if transformed.is_some() {
                    return;
                }
                if let Some(item_enum) = self.transform_item_enum(item_enum) {
                    let is_lifetime = item_enum.potential_lifetimes();
                    let is_unsized = item_enum.size_assumption.is_none()
                        || matches!(item_enum.size_assumption, Some(ElementSize::Unsized));
                    *transformed = Some(ItemEnumTransformed {
                        item_enum,
                        is_lifetime,
                        is_unsized,
                    });
                }
            }
            // Item::Fn(_) => {}
            // Item::Mod(_) => {}
            // Item::Static(_) => {}
            SynItemWithContext::Struct {
                item_struct,
                transformed,
            } => {
                if transformed.is_some() {
                    return;
                }
                if let Some(item_struct) = self.transform_item_struct(item_struct) {
                    let is_lifetime = item_struct.potential_lifetimes();
                    let is_unsized = item_struct.size_assumption.is_none()
                        || matches!(item_struct.size_assumption, Some(ElementSize::Unsized));
                    *transformed = Some(ItemStructTransformed {
                        item_struct,
                        is_lifetime,
                        is_unsized,
                    });
                }
            }
            SynItemWithContext::ApiLevel {
                item_trait,
                transformed,
            } => {
                if transformed.is_some() {
                    return;
                }
                if let Some(api_level) = self.transform_api_level(item_trait) {
                    *transformed = Some(api_level);
                }
            }
            SynItemWithContext::Const {
                item_const,
                transformed,
            } => {
                if transformed.is_some() {
                    return;
                }
                if let Some(item_const) = self.transform_const(item_const) {
                    *transformed = Some(item_const);
                }
            }
        }
    }

    fn transform_item_enum(&mut self, item_enum: &syn::ItemEnum) -> Option<ItemEnum> {
        let mut variants = vec![];
        let mut current_discriminant: u32 = 0;
        let mut max_discriminant: u32 = 0;
        let mut bail = false;
        for variant in &item_enum.variants {
            let discriminant = match self.get_discriminant(variant) {
                Ok(Some(discriminant)) => {
                    current_discriminant = discriminant;
                    discriminant
                }
                Ok(None) => {
                    let d = current_discriminant;
                    current_discriminant = current_discriminant.saturating_add(1);
                    d
                }
                Err(_) => {
                    return None;
                }
            };
            max_discriminant = max_discriminant.max(discriminant);
            let path = FieldPath::new(FieldPathRoot::EnumVariant(variant.ident.clone()));
            match self.convert_fields(&variant.fields, &path) {
                Some(fields) => {
                    let mut attrs = variant.attrs.clone();
                    let since = take_since_attr(&mut attrs);
                    let docs = collect_docs_attrs(&mut attrs);
                    collect_unknown_attributes(&mut attrs, self.messages);
                    variants.push(Variant {
                        docs,
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
        let mut explicit_ww_repr = true;
        let repr = take_ww_repr_attr(&mut attrs, self.messages).unwrap_or_else(|| {
            explicit_ww_repr = false;
            Default::default()
        });
        if max_discriminant > repr.max_discriminant() {
            self.messages
                .push_conversion_error(SynConversionError::EnumDiscriminantNotLargeEnough);
            return None;
        }
        if bail {
            None
        } else {
            let size_assumption = take_size_assumption(&mut attrs);
            let mut docs = collect_docs_attrs(&mut attrs);
            add_notes(&mut docs, size_assumption, true);
            let derive = take_derive_attr(&mut attrs, self.messages);
            collect_unknown_attributes(&mut attrs, self.messages);
            Some(ItemEnum {
                docs,
                derive,
                ident: (&item_enum.ident).into(),
                repr,
                explicit_ww_repr,
                variants,
                size_assumption,
                cfg: None,
            })
        }
    }

    fn get_discriminant(&mut self, variant: &syn::Variant) -> Result<Option<u32>, ()> {
        variant
            .discriminant
            .as_ref()
            .map(|(_, expr)| {
                if let Expr::Lit(lit) = expr {
                    if let Lit::Int(lit_int) = &lit.lit {
                        let d = lit_int.base10_parse().unwrap();
                        Ok(Some(d))
                    } else {
                        self.messages
                            .push_conversion_error(SynConversionError::WrongDiscriminant);
                        Err(())
                    }
                } else {
                    self.messages
                        .push_conversion_error(SynConversionError::WrongDiscriminant);
                    Err(())
                }
            })
            .unwrap_or(Ok(None))
    }

    fn convert_fields(&mut self, fields: &syn::Fields, path: &FieldPath) -> Option<Fields> {
        match fields {
            syn::Fields::Named(fields_named) => {
                let mut named = vec![];
                let mut explicit_flags = vec![];
                for (def_order_idx, field_syn) in fields_named.named.iter().enumerate() {
                    let (field, is_explicit_flag) =
                        self.transform_field(def_order_idx as u32, field_syn, path)?;
                    if is_explicit_flag {
                        explicit_flags.push(field_syn.ident.clone().unwrap().into());
                    }
                    named.push(field)
                }
                create_flags(&mut named, &explicit_flags);
                propagate_default_to_flags(&mut named, self.messages);
                change_is_ok_to_is_some(&mut named);
                Some(Fields::Named(named))
            }
            syn::Fields::Unnamed(fields_unnamed) => {
                let mut unnamed = vec![];
                for (def_order_idx, field) in fields_unnamed.unnamed.iter().enumerate() {
                    let (field, _is_explicit_flag) =
                        self.transform_field(def_order_idx as u32, field, path)?;
                    // TODO: Do unnamed fields have to have since, id, default, etc?
                    // TODO: explicit flags in unnamed fields?
                    unnamed.push(field.ty);
                }
                Some(Fields::Unnamed(unnamed))
            }
            syn::Fields::Unit => Some(Fields::Unit),
        }
    }

    pub fn transform_item_struct(&mut self, item_struct: &syn::ItemStruct) -> Option<ItemStruct> {
        let mut fields = vec![];
        let mut explicit_flags = vec![];
        let mut bail = false;
        for (def_order_idx, field_syn) in item_struct.fields.iter().enumerate() {
            let path = FieldPath::new(FieldPathRoot::NamedField(field_syn.ident.clone().unwrap()));
            match self.transform_field(def_order_idx as u32, field_syn, &path) {
                Some((field, is_explicit_flag)) => {
                    if is_explicit_flag {
                        explicit_flags.push(field_syn.ident.clone().unwrap().into());
                    }
                    fields.push(field);
                }
                None => bail = true,
            }
        }
        if bail {
            None
        } else {
            let mut attrs = item_struct.attrs.clone();
            let size_assumption = take_size_assumption(&mut attrs);
            let mut docs = collect_docs_attrs(&mut attrs);
            add_notes(&mut docs, size_assumption, false);
            let derive = take_derive_attr(&mut attrs, self.messages);
            collect_unknown_attributes(&mut attrs, self.messages);
            create_flags(&mut fields, &explicit_flags);
            propagate_default_to_flags(&mut fields, self.messages);
            change_is_ok_to_is_some(&mut fields);
            Some(ItemStruct {
                docs,
                derive,
                ident: (&item_struct.ident).into(),
                size_assumption,
                fields,
                cfg: None,
            })
        }
    }

    fn transform_field(
        &mut self,
        def_order_idx: u32,
        field: &syn::Field,
        path: &FieldPath,
    ) -> Option<(Field, bool)> {
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

        let ty = self.transform_type(field.ty, Some(&mut field.attrs), &path)?;
        let default = take_default_attr(&mut field.attrs, self.messages);
        let flag = take_flag_attr(&mut field.attrs);
        let id = take_id_attr(&mut field.attrs).unwrap_or(def_order_idx);
        let docs = collect_docs_attrs(&mut field.attrs);
        collect_unknown_attributes(&mut field.attrs, self.messages);

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

            Some((
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
            Some((
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

    fn transform_type(
        &mut self,
        ty: syn::Type,
        attrs: Option<&mut Vec<Attribute>>,
        path: &FieldPath,
    ) -> Option<Type> {
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
                        "unib32" | "UNib32" => Type::UNib32,
                        "uleb32" | "ULeb32" => Type::ULeb32,
                        "uleb64" | "ULeb63" => Type::ULeb64,
                        "uleb128" | "ULeb128" => Type::ULeb128,
                        "i8" => Type::I8,
                        "i16" => Type::I16,
                        "i32" => Type::I32,
                        "i64" => Type::I64,
                        "i128" => Type::I128,
                        "ileb32" | "ILeb32" => Type::ILeb32,
                        "ileb64" | "ILeb64" => Type::ILeb64,
                        "ileb128" | "ILeb128" => Type::ILeb128,
                        "f32" => Type::F32,
                        "f64" => Type::F64,
                        "String" | "str" => Type::String,
                        "Vec" | "RefVec" => return self.transform_type_vec(path_segment, path),
                        "Result" => return self.transform_type_result(path_segment, path),
                        "Option" => return self.transform_type_option(path_segment, path),
                        "DateTime" => {
                            Type::Sized(Path::new_ident((&path_segment.ident).into()), false)
                        }
                        other_ty => {
                            // u1, u2, .., u63, except u8, u16, ...
                            if let Some(un) = other_ty
                                .strip_prefix('U')
                                .or_else(|| other_ty.strip_prefix('u'))
                                .or_else(|| other_ty.strip_prefix('I'))
                                .or_else(|| other_ty.strip_prefix('i'))
                            {
                                let bits: Result<u8, _> = un.parse();
                                if let Ok(bits) = bits {
                                    if (1..=63).contains(&bits) {
                                        return Some(Type::Sized(
                                            Path::new_ident(Ident::new(other_ty)),
                                            false,
                                        ));
                                    }
                                }
                            }

                            let mut is_lifetime = false;
                            if let PathArguments::AngleBracketed(args) = &path_segment.arguments {
                                let mut args = args.args.iter();
                                if let Some(arg) = args.next() {
                                    is_lifetime = matches!(arg, GenericArgument::Lifetime(_));
                                }
                            }

                            for item in &self.current_file.items {
                                if item.ident().map(|ident| ident == other_ty).unwrap_or(false) {
                                    let is_lifetime_is_unsized = match item {
                                        SynItemWithContext::Enum { transformed, .. } => transformed
                                            .as_ref()
                                            .map(|t| (t.is_lifetime, t.is_unsized)),
                                        SynItemWithContext::Struct { transformed, .. } => {
                                            transformed
                                                .as_ref()
                                                .map(|t| (t.is_lifetime, t.is_unsized))
                                        }
                                        SynItemWithContext::ApiLevel { .. } => unreachable!(),
                                        SynItemWithContext::Const { .. } => unreachable!(),
                                    };
                                    // if is_lifetime is None, one more pass is needed
                                    return is_lifetime_is_unsized.map(
                                        |(is_lifetime, is_unsized)| {
                                            if is_unsized {
                                                Type::Unsized(
                                                    Path::new_ident(Ident::new(other_ty)),
                                                    is_lifetime,
                                                )
                                            } else {
                                                Type::Sized(
                                                    Path::new_ident(Ident::new(other_ty)),
                                                    is_lifetime,
                                                )
                                            }
                                        },
                                    );
                                }
                            }
                            return Some(Type::Unsized(
                                Path::new_ident(Ident::new(other_ty)),
                                is_lifetime,
                            ));
                        }
                    };
                    Some(ty)
                } else {
                    // go through files and find it
                    self.messages
                        .push_conversion_error(SynConversionError::UnknownType(format!(
                            "{type_path:?}"
                        )));
                    None
                }
            }
            syn::Type::Reference(type_ref) => {
                let mut ty = self.transform_type(type_ref.elem.as_ref().clone(), attrs, path)?;
                match &mut ty {
                    Type::Unsized(_, lifetime) | Type::Sized(_, lifetime) => {
                        *lifetime = true;
                    }
                    _ => {}
                }
                Some(ty)
            }
            syn::Type::Array(_type_array) => {
                unimplemented!("collect_and_convert: array")
            }
            u => {
                self.messages
                    .push_conversion_error(SynConversionError::UnknownType(format!("{u:?}")));
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
                    "expected Result<T, E>, got {arg:?}"
                )));
            return None;
        };
        let ok_path = path.clone_and_push(FieldSelector::ResultIfOk);
        let ok_ty = self.transform_type(ok_ty.clone(), None, &ok_path)?;
        let err_path = path.clone_and_push(FieldSelector::ResultIfErr);
        let err_ty = self.transform_type(err_ty.clone(), None, &err_path)?;
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
        let mut args = arg.args.iter();
        let Some(arg) = args.next() else {
            self.messages
                .push_conversion_error(SynConversionError::UnsupportedType(
                    "expected Vec<T>, got Vec<T, ?>".into(),
                ));
            return None;
        };
        let arg = if matches!(arg, GenericArgument::Lifetime(_)) {
            let Some(arg) = args.next() else {
                self.messages
                    .push_conversion_error(SynConversionError::UnsupportedType(
                        "expected Vec<'i, T>, got Vec<'i, T, ?>".into(),
                    ));
                return None;
            };
            arg
        } else {
            arg
        };
        let GenericArgument::Type(inner_ty) = arg else {
            self.messages
                .push_conversion_error(SynConversionError::UnsupportedType(format!(
                    "expected Vec<T>, got {arg:?}"
                )));
            return None;
        };
        let inner_ty = self.transform_type(inner_ty.clone(), None, path)?;
        Some(Type::Vec(Layout::Builtin(Box::new(inner_ty))))
    }

    fn transform_type_option(
        &mut self,
        path_segment: &PathSegment,
        path: &FieldPath,
    ) -> Option<Type> {
        let PathArguments::AngleBracketed(arg) = &path_segment.arguments else {
            self.messages
                .push_conversion_error(SynConversionError::UnsupportedType(
                    "expected Option<T>, got Option or Option()".into(),
                ));
            return None;
        };
        let Some(arg) = arg.args.first() else {
            self.messages
                .push_conversion_error(SynConversionError::UnsupportedType(
                    "expected Option<T>, got Option<T, ?>".into(),
                ));
            return None;
        };
        let GenericArgument::Type(inner_ty) = arg else {
            self.messages
                .push_conversion_error(SynConversionError::UnsupportedType(format!(
                    "expected Option<T>, got {arg:?}"
                )));
            return None;
        };
        let path = path.clone_and_push(FieldSelector::OptionIsSome);
        let inner_ty = self.transform_type(inner_ty.clone(), None, &path)?;
        let flag_ident = path.flag_ident().into();
        Some(Type::Option(flag_ident, Box::new(inner_ty)))
    }

    fn transform_return_type(&mut self, ty: ReturnType, path: &FieldPath) -> Option<Type> {
        match ty {
            ReturnType::Default => None,
            ReturnType::Type(_, ty) => self.transform_type(*ty, None, path),
        }
    }

    fn transform_api_level(&mut self, item_trait: &syn::ItemTrait) -> Option<ApiLevel> {
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
                        let ty = self.transform_type(
                            pat_type.ty.deref().clone(),
                            None,
                            &FieldPath::new(FieldPathRoot::Argument(arg_ident.ident.clone())),
                        )?;
                        args.push(Argument {
                            ident: (&arg_ident.ident).into(),
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
                    collect_unknown_attributes(&mut attrs, self.messages);
                    items.push(ApiItem {
                        id,
                        docs,
                        multiplicity: Multiplicity::Flat,
                        kind: ApiItemKind::Method {
                            ident: (&trait_item_fn.sig.ident).into(),
                            args,
                            return_type: self.transform_return_type(
                                trait_item_fn.sig.output.clone(),
                                &FieldPath::new(FieldPathRoot::Output),
                            ),
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
                    let stream_args: StreamMacroArgs =
                        syn::parse2(item_macro.mac.tokens.clone()).unwrap();
                    let path = FieldPath::new(FieldPathRoot::NamedField(stream_args.ident.clone())); // TODO: Clarify FieldPath purpose
                    let docs = collect_docs_attrs(&mut attrs);
                    if kind == "stream_up" || kind == "stream_down" {
                        let is_up = kind == "stream_up";
                        items.push(ApiItem {
                            id,
                            docs,
                            multiplicity: Multiplicity::Flat,
                            kind: ApiItemKind::Stream {
                                ident: stream_args.ident.into(),
                                // ty: Type::Unsized(Path::new_ident(stream_args.ty_name.into()), false),
                                ty: self.transform_type(stream_args.ty, None, &path)?,
                                is_up,
                            },
                        });
                    } else if kind == "property" {
                        items.push(ApiItem {
                            id,
                            docs,
                            multiplicity: Multiplicity::Flat,
                            kind: ApiItemKind::Property {
                                ident: stream_args.ident.into(),
                                ty: self.transform_type(stream_args.ty, None, &path)?,
                            },
                        });
                    } else {
                        self.messages
                            .push_conversion_error(SynConversionError::UnknownApiResource);
                    }
                    collect_unknown_attributes(&mut attrs, self.messages);
                }
                TraitItem::Verbatim(_) => {}
                _ => {}
            }
        }
        let mut attrs = item_trait.attrs.clone();
        let docs = collect_docs_attrs(&mut attrs);
        Some(ApiLevel { docs, items })
    }

    fn transform_const(&mut self, item_const: &syn::ItemConst) -> Option<ItemConst> {
        let ty = self.transform_type(
            item_const.ty.deref().clone(),
            None,
            &FieldPath::new(FieldPathRoot::Argument(item_const.ident.clone())),
        )?;
        let mut attrs = item_const.attrs.clone();
        let docs = collect_docs_attrs(&mut attrs);
        collect_unknown_attributes(&mut attrs, self.messages);
        Some(ItemConst {
            docs,
            ident: item_const.ident.clone().into(),
            ty,
            value: item_const.expr.deref().clone(),
        })
    }
}

struct StreamMacroArgs {
    ident: syn::Ident,
    _punct: syn::Token![,],
    ty: syn::Type,
}

impl Parse for StreamMacroArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(StreamMacroArgs {
            ident: input.parse()?,
            _punct: input.parse()?,
            ty: input.parse()?,
        })
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
        let flag_ident = Ident::new(format!("_{}_flag", ident.sym));
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

fn propagate_default_to_flags(fields: &mut [Field], messages: &mut Messages) {
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
            messages.messages.push(Message::SynConversionError(
                SynConversionError::DefaultUsedOnNotOption,
            ));
        }
        if default != &Value::None {
            messages.messages.push(Message::SynConversionError(
                SynConversionError::UnsupportedDefaultValue,
            ));
        }
        set_to_default_false.push(f.ident.sym.clone());
    }
    for ident in set_to_default_false {
        for f in fields.iter_mut() {
            if let Type::IsSome(flag_for_ident) = &f.ty {
                if flag_for_ident.sym != ident {
                    continue;
                }
                f.default = Some(Value::Bool(false)); // read is_some flag as false on EOB
            } else if matches!(f.ty, Type::Option(_, _)) && f.ident.sym == ident {
                f.default = None; // TODO: Change to actual default value
            }
        }
    }
    if default_is_not_last {
        messages.messages.push(Message::SynConversionError(
            SynConversionError::WrongEvolvedTypeOrder,
        ));
    }
}

/// Change IsOk to IsSome for explicit flags, as full field list is needed to determine which one to use.
fn change_is_ok_to_is_some(fields: &mut [Field]) {
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

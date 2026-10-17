use super::{
    crate_walker::{CrateContext, Scratch, TypeInProgress},
    util::{
        collect_docs, derive_shrink_wrap_args, get_since_attr, use_tree_globs, use_tree_path_to,
    },
};
use anyhow::{Context, Result, anyhow};
use shrink_wrap::{ElementSize, UNib32};
use syn::{
    Attribute, Expr, Fields, GenericArgument, Item, ItemEnum, ItemStruct, Lit, Meta, PathArguments,
    PathSegment, Type, TypePath,
};
use ww_numeric::NumericValue;
use ww_numeric::{IBits, NumericAnyTypeOwned, UBits};
use ww_self::{
    FieldOwned, FieldsOwned, GenericArgOwned, GenericParamKindOwned, GenericParamOwned,
    ItemEnumOwned, ItemStructOwned, Length, NumericBaseType, Repr, TypeLocationOwned, TypeOwned,
    ValueOwned, VariantOwned,
};

pub(crate) fn convert_ty(
    ty: &Type,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<TypeOwned> {
    match ty {
        Type::Array(type_array) => {
            let len = convert_length(&type_array.len, current_crate, scratch).ok_or_else(|| {
                anyhow!(
                    "array length must be an integer literal, a const of one in lib.rs or a const parameter"
                )
                .context(current_crate.err_context())
            })?;
            let inner = convert_ty(&type_array.elem, current_crate, scratch)?;
            Ok(TypeOwned::Array {
                len,
                ty: Box::new(inner),
            })
        }
        Type::Path(type_path) => convert_ty_path(type_path, current_crate, scratch),
        Type::Reference(type_ref) => convert_ty(type_ref.elem.as_ref(), current_crate, scratch),
        Type::Slice(type_slice) => {
            let ty = convert_ty(&type_slice.elem, current_crate, scratch)?;
            Ok(TypeOwned::Vec(Box::new(ty)))
        }
        Type::Tuple(type_tuple) => {
            let mut types = vec![];
            for elem in &type_tuple.elems {
                types.push(convert_ty(elem, current_crate, scratch)?);
            }
            Ok(TypeOwned::Tuple(types))
        }
        u => Err(anyhow!("Unsupported type {u:?}")),
    }
}

fn numeric_base(ty: NumericBaseType) -> TypeOwned {
    TypeOwned::NumericAny(NumericAnyTypeOwned::Base(ty))
}

pub(crate) fn convert_ty_path(
    ty_path: &TypePath,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<TypeOwned> {
    let segments: Vec<_> = ty_path.path.segments.iter().collect();
    let Some((last, leading)) = segments.split_last() else {
        return Err(anyhow!("Empty type path"));
    };
    // a parameter of the generic definition being converted
    if leading.is_empty()
        && ty_path.qself.is_none()
        && matches!(last.arguments, PathArguments::None)
        && let Some((idx, param)) = scope_param(scratch, &last.ident.to_string())
    {
        return match param.kind {
            GenericParamKindOwned::Type => Ok(TypeOwned::Param(UNib32(idx))),
            GenericParamKindOwned::Const { .. } => {
                Err(anyhow!("const parameter {} is used as a type", param.ident)
                    .context(current_crate.err_context()))
            }
        };
    }
    if leading.is_empty() {
        convert_ty_path_segment(last, current_crate, scratch)
    } else {
        let cx = current_crate.resolve_path(leading.iter().map(|s| &s.ident), scratch)?;
        convert_ty_path_segment(last, &cx, scratch)
    }
}

pub(crate) fn convert_ty_path_segment(
    segment: &PathSegment,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<TypeOwned> {
    let ty_name = segment.ident.to_string();
    match ty_name.as_str() {
        "bool" => Ok(TypeOwned::Bool),
        "Nibble" | "nib" => Ok(numeric_base(NumericBaseType::Nibble)),
        "u8" => Ok(numeric_base(NumericBaseType::U8)),
        "u16" => Ok(numeric_base(NumericBaseType::U16)),
        "u32" => Ok(numeric_base(NumericBaseType::U32)),
        "u64" => Ok(numeric_base(NumericBaseType::U64)),
        "u128" => Ok(numeric_base(NumericBaseType::U128)),
        "i8" => Ok(numeric_base(NumericBaseType::I8)),
        "i16" => Ok(numeric_base(NumericBaseType::I16)),
        "i32" => Ok(numeric_base(NumericBaseType::I32)),
        "i64" => Ok(numeric_base(NumericBaseType::I64)),
        "i128" => Ok(numeric_base(NumericBaseType::I128)),
        "UNib32" | "unib32" => Ok(numeric_base(NumericBaseType::UNib32)),
        "UVlq32" | "uvlq32" => Ok(numeric_base(NumericBaseType::UVlq32)),
        "UVlq64" | "uvlq64" => Ok(numeric_base(NumericBaseType::UVlq64)),
        "UN" | "un" => Ok(numeric_base(NumericBaseType::UN)),
        "IN" | "in" => Ok(numeric_base(NumericBaseType::IN)),
        "f16" => Ok(numeric_base(NumericBaseType::F16)),
        "f32" => Ok(numeric_base(NumericBaseType::F32)),
        "f64" => Ok(numeric_base(NumericBaseType::F64)),
        "ULeb32" | "uleb32" => Ok(numeric_base(NumericBaseType::ULeb32)),
        "ULeb64" | "uleb64" => Ok(numeric_base(NumericBaseType::ULeb64)),
        "ULeb128" | "uleb128" => Ok(numeric_base(NumericBaseType::ULeb128)),
        "ILeb32" | "ileb32" => Ok(numeric_base(NumericBaseType::ILeb32)),
        "ILeb64" | "ileb64" => Ok(numeric_base(NumericBaseType::ILeb64)),
        "ILeb128" | "ileb128" => Ok(numeric_base(NumericBaseType::ILeb128)),
        "String" | "str" => Ok(TypeOwned::String),
        "Vec" | "RefVec" => convert_ty_vec(segment, current_crate, scratch),
        "MaxLen" => convert_ty_max_len(segment, current_crate, scratch),
        "Option" => convert_ty_option(segment, current_crate, scratch),
        "Result" => convert_ty_result(segment, current_crate, scratch),
        "Range" => convert_ty_range(segment, current_crate, scratch),
        "RangeInclusive" => convert_ty_range_inclusive(segment, current_crate, scratch),
        "Delta" | "DeltaOwned" | "DeltaOfDelta" | "DeltaOfDeltaOwned" | "XorFloat"
        | "XorFloatOwned" => convert_ty_series(segment, current_crate, scratch),
        // SW-60: the item type in its own columns
        "Series" | "SeriesOwned" => {
            let inner_ty = get_inner_angle_bracketed_ty(segment, current_crate)?;
            Ok(TypeOwned::Series(Box::new(convert_ty(
                inner_ty,
                current_crate,
                scratch,
            )?)))
        }
        "Box" | "RefBox" => convert_ty_ref_box(segment, current_crate, scratch),
        user_ty => {
            if let Some(ty) = convert_ub_ib(user_ty) {
                return Ok(ty);
            }

            let (def_cx, item) = find_user_item(segment, current_crate, scratch)?;
            convert_user_ty(segment, &def_cx, &item, current_crate, scratch)
        }
    }
}

/// Definition of the user type `segment` names, and the crate it is in: defined in `current_crate`, imported by
/// name, or through a glob import (looked into last, as Rust does).
fn find_user_item(
    segment: &PathSegment,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<(CrateContext, Item)> {
    let user_ty = segment.ident.to_string();
    let user_ty = user_ty.as_str();
    for item in &current_crate.lib_rs_ast.items {
        match item {
            Item::Enum(item_enum) if item_enum.ident == user_ty => {
                return Ok((current_crate.clone(), item.clone()));
            }
            Item::Struct(item_struct) if item_struct.ident == user_ty => {
                return Ok((current_crate.clone(), item.clone()));
            }
            Item::Use(item_use) => {
                let Some(path) = use_tree_path_to(&item_use.tree, user_ty) else {
                    continue;
                };
                let dependent_crate = current_crate.resolve_path(path, scratch)?;
                return find_user_item(segment, &dependent_crate, scratch);
            }
            _ => {}
        }
    }
    // `use wire_weaver::prelude::*` brings in `CommonError`: look into glob imports last, as Rust does
    for item in &current_crate.lib_rs_ast.items {
        let Item::Use(item_use) = item else {
            continue;
        };
        for path in use_tree_globs(&item_use.tree) {
            // a glob into a crate that can't be walked (`std`) or that doesn't have the type
            let Ok(dependent_crate) = current_crate.resolve_path(path, scratch) else {
                continue;
            };
            if declares_or_imports(&dependent_crate, user_ty) {
                return find_user_item(segment, &dependent_crate, scratch);
            }
        }
    }
    Err(anyhow!("Type {segment:?} not found").context(current_crate.err_context()))
}

/// The user type `segment` names, defined as `item` in `def_cx`: an out-of-line reference, or an instance of a
/// generic definition with the arguments of `segment` (converted in `current_crate`, in the current generic scope).
fn convert_user_ty(
    segment: &PathSegment,
    def_cx: &CrateContext,
    item: &Item,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<TypeOwned> {
    let (ident, generics) = match item {
        Item::Struct(item_struct) => (&item_struct.ident, &item_struct.generics),
        Item::Enum(item_enum) => (&item_enum.ident, &item_enum.generics),
        _ => return Err(anyhow!("{segment:?} is not a struct or an enum")),
    };
    let params = convert_generic_params(ident, generics, def_cx, scratch)?;
    let args = convert_generic_args(segment, ident, &params, def_cx, current_crate, scratch)?;
    let params: Vec<_> = params.into_iter().map(|(p, _)| p).collect();
    let ty = convert_def(def_cx, item, params, scratch)?;
    if args.is_empty() {
        return Ok(ty);
    }
    let TypeOwned::OutOfLine { type_idx } = ty else {
        return Err(anyhow!(
            "internal: generic definition {ident} is not out-of-line"
        ));
    };
    Ok(TypeOwned::Instance { type_idx, args })
}

/// Converts the definition of a struct or enum (generic or not, without arguments) into the types array, e.g. for
/// a crate snapshot. Returns its index.
pub(crate) fn convert_named_def(
    ident: &syn::Ident,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<UNib32> {
    let segment = PathSegment::from(ident.clone());
    let (def_cx, item) = find_user_item(&segment, current_crate, scratch)?;
    let generics = match &item {
        Item::Struct(item_struct) => &item_struct.generics,
        Item::Enum(item_enum) => &item_enum.generics,
        _ => return Err(anyhow!("{ident} is not a struct or an enum")),
    };
    let params = convert_generic_params(ident, generics, &def_cx, scratch)?;
    let params = params.into_iter().map(|(p, _)| p).collect();
    match convert_def(&def_cx, &item, params, scratch)? {
        TypeOwned::OutOfLine { type_idx } => Ok(type_idx),
        _ => Err(anyhow!(
            "internal: definition of {ident} is not out-of-line"
        )),
    }
}

/// `item` with `params` in scope while its fields are converted.
///
/// A type that refers to itself (through `RefBox`, `Vec`, `Option`, ..., directly or through other types) gets its
/// index in the types array reserved when the reference is found, the reference points there. Generic definitions
/// can't be part of such a cycle (each round could need a new instance, API-43).
fn convert_def(
    def_cx: &CrateContext,
    item: &Item,
    params: Vec<GenericParamOwned>,
    scratch: &mut Scratch,
) -> Result<TypeOwned> {
    let ty_name = match item {
        Item::Enum(item_enum) => item_enum.ident.to_string(),
        Item::Struct(item_struct) => item_struct.ident.to_string(),
        _ => return Err(anyhow!("not a struct or an enum")),
    };
    let key = (def_cx.version().clone(), ty_name.clone());
    if let Some(type_idx) = scratch.recursive_types.get(&key) {
        return Ok(TypeOwned::OutOfLine {
            type_idx: *type_idx,
        });
    }
    if let Some(pos) = scratch.types_in_progress.iter().position(|t| t.key == key) {
        return refer_back(def_cx, scratch, pos);
    }
    scratch.types_in_progress.push(TypeInProgress {
        key,
        generic: !params.is_empty(),
        slot: None,
    });
    let saved_scope = std::mem::replace(&mut scratch.generic_scope, params);
    let ty = match item {
        Item::Enum(item_enum) => convert_item_enum(def_cx, scratch, ty_name, item_enum),
        Item::Struct(item_struct) => convert_item_struct(def_cx, scratch, ty_name, item_struct),
        _ => unreachable!(),
    };
    scratch.generic_scope = saved_scope;
    let in_progress = scratch
        .types_in_progress
        .pop()
        .expect("internal: types in progress out of balance");
    let ty = ty?;
    match in_progress.slot {
        Some(type_idx) => {
            let crate_idx = scratch.root_bundle.find_crate_or_create(def_cx);
            scratch.root_bundle.types[type_idx.0 as usize] =
                TypeLocationOwned::InLine { ty, crate_idx };
            scratch.recursive_types.insert(in_progress.key, type_idx);
            Ok(TypeOwned::OutOfLine { type_idx })
        }
        None => {
            if let Some(type_idx) = scratch.root_bundle.find_type(&ty) {
                return Ok(TypeOwned::OutOfLine { type_idx });
            }
            Ok(scratch.root_bundle.push_out_of_line(ty, def_cx))
        }
    }
}

/// A reference to `types_in_progress[pos]` while it is converted: its reserved index. Errors out when the cycle
/// goes through a generic definition.
fn refer_back(def_cx: &CrateContext, scratch: &mut Scratch, pos: usize) -> Result<TypeOwned> {
    let cycle = &scratch.types_in_progress[pos..];
    if let Some(generic) = cycle.iter().find(|t| t.generic) {
        let names: Vec<&str> = cycle.iter().map(|t| t.key.1.as_str()).collect();
        return Err(anyhow!(
            "Generic type {} refers to itself (through {}), only types without parameters can",
            generic.key.1,
            names.join(" -> ")
        )
        .context(def_cx.err_context()));
    }
    let slot = match scratch.types_in_progress[pos].slot {
        Some(slot) => slot,
        None => {
            // filled once the definition is converted
            let slot = UNib32(scratch.root_bundle.types.len() as u32);
            let crate_idx = scratch.root_bundle.find_crate_or_create(def_cx);
            scratch.root_bundle.types.push(TypeLocationOwned::InLine {
                ty: TypeOwned::Tuple(vec![]),
                crate_idx,
            });
            scratch.types_in_progress[pos].slot = Some(slot);
            slot
        }
    };
    Ok(TypeOwned::OutOfLine { type_idx: slot })
}

/// Type and const parameters of the definition `ident` (lifetimes are left out), each with its default as written.
fn convert_generic_params(
    ident: &syn::Ident,
    generics: &syn::Generics,
    def_cx: &CrateContext,
    scratch: &mut Scratch,
) -> Result<Vec<(GenericParamOwned, Option<ParamDefault>)>> {
    let mut params = vec![];
    for param in &generics.params {
        match param {
            syn::GenericParam::Lifetime(_) => {}
            syn::GenericParam::Type(type_param) => params.push((
                GenericParamOwned {
                    ident: type_param.ident.to_string(),
                    kind: GenericParamKindOwned::Type,
                },
                type_param
                    .default
                    .clone()
                    .map(|(_, ty)| ParamDefault::Type(Box::new(ty))),
            )),
            syn::GenericParam::Const(const_param) => {
                let ty = convert_const_param_ty(&const_param.ty).ok_or_else(|| {
                    anyhow!(
                        "{ident}: const parameter {} must be a bool or an integer",
                        const_param.ident
                    )
                    .context(def_cx.err_context())
                })?;
                params.push((
                    GenericParamOwned {
                        ident: const_param.ident.to_string(),
                        kind: GenericParamKindOwned::Const { ty },
                    },
                    const_param
                        .default
                        .clone()
                        .map(|(_, expr)| ParamDefault::Const(Box::new(expr))),
                ));
            }
        }
    }
    let _ = scratch;
    Ok(params)
}

/// Default of a parameter, `struct A<T = u8, const N: usize = 4>`.
enum ParamDefault {
    Type(Box<Type>),
    Const(Box<Expr>),
}

/// Type of a const parameter: `bool` or an integer, `usize` as `u64`, `isize` as `i64`.
fn convert_const_param_ty(ty: &Type) -> Option<TypeOwned> {
    let Type::Path(type_path) = ty else {
        return None;
    };
    let base = match type_path.path.get_ident()?.to_string().as_str() {
        "bool" => return Some(TypeOwned::Bool),
        "u8" => NumericBaseType::U8,
        "u16" => NumericBaseType::U16,
        "u32" => NumericBaseType::U32,
        "u64" | "usize" => NumericBaseType::U64,
        "u128" => NumericBaseType::U128,
        "i8" => NumericBaseType::I8,
        "i16" => NumericBaseType::I16,
        "i32" => NumericBaseType::I32,
        "i64" | "isize" => NumericBaseType::I64,
        "i128" => NumericBaseType::I128,
        _ => return None,
    };
    Some(numeric_base(base))
}

/// Arguments of `segment` for the parameters of `ident`: types and const values converted in `current_crate` with
/// the current generic scope, missing trailing ones from the parameters' defaults (converted in `def_cx`).
fn convert_generic_args(
    segment: &PathSegment,
    ident: &syn::Ident,
    params: &[(GenericParamOwned, Option<ParamDefault>)],
    def_cx: &CrateContext,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<Vec<GenericArgOwned>> {
    let err = |msg: String| anyhow!("{ident}: {msg}").context(current_crate.err_context());
    let written: Vec<&GenericArgument> = match &segment.arguments {
        PathArguments::None => vec![],
        PathArguments::AngleBracketed(args) => args
            .args
            .iter()
            .filter(|a| !matches!(a, GenericArgument::Lifetime(_)))
            .collect(),
        PathArguments::Parenthesized(_) => return Err(err("expected <..> arguments".into())),
    };
    if written.len() > params.len() {
        return Err(err(format!(
            "has {} type and const parameters, got {} arguments",
            params.len(),
            written.len()
        )));
    }
    let mut args = Vec::with_capacity(params.len());
    for (idx, (param, default)) in params.iter().enumerate() {
        let arg = match (written.get(idx), default) {
            (Some(arg), _) => convert_generic_arg(arg, param, current_crate, scratch)
                .map_err(|e| e.context(format!("argument {} of {ident}", param.ident)))?,
            (None, Some(default)) => {
                // the default is written in the definition's crate, where the caller's parameters are not in scope
                let saved = std::mem::take(&mut scratch.generic_scope);
                let arg = match default {
                    ParamDefault::Type(ty) => {
                        convert_ty(ty, def_cx, scratch).map(GenericArgOwned::Type)
                    }
                    ParamDefault::Const(expr) => {
                        convert_const_arg(expr, param, def_cx, scratch).map(GenericArgOwned::Const)
                    }
                };
                scratch.generic_scope = saved;
                arg.map_err(|e| e.context(format!("default of {} of {ident}", param.ident)))?
            }
            (None, None) => {
                return Err(err(format!("missing argument for {}", param.ident)));
            }
        };
        args.push(arg);
    }
    Ok(args)
}

fn convert_generic_arg(
    arg: &GenericArgument,
    param: &GenericParamOwned,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<GenericArgOwned> {
    match (&param.kind, arg) {
        (GenericParamKindOwned::Type, GenericArgument::Type(ty)) => Ok(GenericArgOwned::Type(
            convert_ty(ty, current_crate, scratch)?,
        )),
        (GenericParamKindOwned::Const { .. }, GenericArgument::Const(expr)) => {
            const_param_or_value(expr, param, current_crate, scratch)
        }
        // `N` and `SIZE` alone parse as types
        (GenericParamKindOwned::Const { .. }, GenericArgument::Type(Type::Path(type_path)))
            if type_path.path.get_ident().is_some() =>
        {
            let expr = Expr::Path(syn::ExprPath {
                attrs: vec![],
                qself: None,
                path: type_path.path.clone(),
            });
            const_param_or_value(&expr, param, current_crate, scratch)
        }
        _ => Err(anyhow!(
            "expected a {}",
            match param.kind {
                GenericParamKindOwned::Type => "type",
                GenericParamKindOwned::Const { .. } => "const value",
            }
        )),
    }
}

/// A const parameter of the definition being converted passed on, or a value.
fn const_param_or_value(
    expr: &Expr,
    param: &GenericParamOwned,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<GenericArgOwned> {
    if let Expr::Path(path) = expr
        && let Some(name) = path.path.get_ident()
        && let Some((idx, _)) = scope_param(scratch, &name.to_string())
    {
        return Ok(GenericArgOwned::ConstParam(UNib32(idx)));
    }
    Ok(GenericArgOwned::Const(convert_const_arg(
        expr,
        param,
        current_crate,
        scratch,
    )?))
}

/// Value of a const argument: an integer or bool literal (also `-1`, `{ 4 }`), or a `const` of one in lib.rs.
fn convert_const_arg(
    expr: &Expr,
    param: &GenericParamOwned,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<ValueOwned> {
    let GenericParamKindOwned::Const { ty } = &param.kind else {
        return Err(anyhow!("{} is not a const parameter", param.ident));
    };
    let value = const_expr_value(expr, current_crate).ok_or_else(|| {
        anyhow!("expected an integer or bool literal, or a const of one in lib.rs")
    })?;
    let _ = scratch;
    match (ty, value) {
        (TypeOwned::Bool, ConstValue::Bool(b)) => Ok(ValueOwned::Bool(b)),
        (TypeOwned::NumericAny(NumericAnyTypeOwned::Base(base)), ConstValue::Int(v)) => {
            Ok(ValueOwned::Numeric(numeric_value(base, v)?))
        }
        (ty, value) => Err(anyhow!("{value:?} is not a value of {ty:?}")),
    }
}

#[derive(Debug, Clone, Copy)]
enum ConstValue {
    Bool(bool),
    Int(i128),
}

fn const_expr_value(expr: &Expr, current_crate: &CrateContext) -> Option<ConstValue> {
    match expr {
        Expr::Lit(lit) => match &lit.lit {
            Lit::Int(lit_int) => lit_int.base10_parse().ok().map(ConstValue::Int),
            Lit::Bool(lit_bool) => Some(ConstValue::Bool(lit_bool.value)),
            _ => None,
        },
        Expr::Unary(syn::ExprUnary {
            op: syn::UnOp::Neg(_),
            expr,
            ..
        }) => match const_expr_value(expr, current_crate)? {
            ConstValue::Int(v) => Some(ConstValue::Int(v.checked_neg()?)),
            ConstValue::Bool(_) => None,
        },
        Expr::Block(block) => match block.block.stmts.as_slice() {
            [syn::Stmt::Expr(expr, None)] => const_expr_value(expr, current_crate),
            _ => None,
        },
        Expr::Group(group) => const_expr_value(&group.expr, current_crate),
        Expr::Paren(paren) => const_expr_value(&paren.expr, current_crate),
        Expr::Path(path) => {
            let name = path.path.get_ident()?.to_string();
            current_crate
                .lib_rs_ast
                .items
                .iter()
                .find_map(|item| match item {
                    Item::Const(item_const) if item_const.ident == name => {
                        const_expr_value(&item_const.expr, current_crate)
                    }
                    _ => None,
                })
        }
        _ => None,
    }
}

/// `v` as a value of the integer type `base`, an error when it doesn't fit.
fn numeric_value(base: &NumericBaseType, v: i128) -> Result<NumericValue> {
    use NumericBaseType::*;
    let out_of_range = || anyhow!("{v} is out of range of {base:?}");
    Ok(match base {
        U8 => NumericValue::U8(v.try_into().map_err(|_| out_of_range())?),
        U16 => NumericValue::U16(v.try_into().map_err(|_| out_of_range())?),
        U32 => NumericValue::U32(v.try_into().map_err(|_| out_of_range())?),
        U64 => NumericValue::U64(v.try_into().map_err(|_| out_of_range())?),
        U128 => NumericValue::U128(v.try_into().map_err(|_| out_of_range())?),
        I8 => NumericValue::I8(v.try_into().map_err(|_| out_of_range())?),
        I16 => NumericValue::I16(v.try_into().map_err(|_| out_of_range())?),
        I32 => NumericValue::I32(v.try_into().map_err(|_| out_of_range())?),
        I64 => NumericValue::I64(v.try_into().map_err(|_| out_of_range())?),
        I128 => NumericValue::I128(v),
        _ => return Err(anyhow!("{base:?} can't be the type of a const parameter")),
    })
}

/// Index and definition of the parameter `name` of the generic definition being converted.
fn scope_param<'s>(scratch: &'s Scratch, name: &str) -> Option<(u32, &'s GenericParamOwned)> {
    scratch
        .generic_scope
        .iter()
        .enumerate()
        .find(|(_, p)| p.ident == name)
        .map(|(idx, p)| (idx as u32, p))
}

/// Array length or `MaxLen` bound: an integer literal (also as `{ 64 }`), a `const` of one in lib.rs, or an integer
/// const parameter of the definition being converted.
fn convert_length(expr: &Expr, current_crate: &CrateContext, scratch: &Scratch) -> Option<Length> {
    if let Expr::Path(path) = expr
        && let Some(name) = path.path.get_ident()
        && let Some((idx, param)) = scope_param(scratch, &name.to_string())
    {
        return matches!(param.kind, GenericParamKindOwned::Const { .. })
            .then_some(Length::Param(UNib32(idx)));
    }
    if let Expr::Block(block) = expr
        && let [syn::Stmt::Expr(expr, None)] = block.block.stmts.as_slice()
    {
        return convert_length(expr, current_crate, scratch);
    }
    match const_expr_value(expr, current_crate)? {
        ConstValue::Int(v) => u32::try_from(v).ok().map(|v| Length::Fixed(UNib32(v))),
        ConstValue::Bool(_) => None,
    }
}

/// Whether `name` is defined in `cx` or imported into it by name (globs are not followed, to not walk in circles).
fn declares_or_imports(cx: &CrateContext, name: &str) -> bool {
    cx.lib_rs_ast.items.iter().any(|item| match item {
        Item::Enum(item_enum) => item_enum.ident == name,
        Item::Struct(item_struct) => item_struct.ident == name,
        Item::Use(item_use) => use_tree_path_to(&item_use.tree, name).is_some(),
        _ => false,
    })
}

fn convert_ub_ib(user_ty: &str) -> Option<TypeOwned> {
    // u1, u2, .., u64, i2, i3, .., i64
    let user_ty = user_ty.to_lowercase();
    let xn = user_ty
        .strip_prefix("ub")
        .or_else(|| user_ty.strip_prefix("u"))
        .or_else(|| user_ty.strip_prefix("ib"))
        .or_else(|| user_ty.strip_prefix("i"))?;
    let bits: Result<u8, _> = xn.parse();
    if let Ok(bits) = bits
        && user_ty.starts_with('u')
        && (1..=64).contains(&bits)
    {
        return Some(numeric_base(NumericBaseType::UB(UBits(bits))));
    }
    if let Ok(bits) = bits
        && user_ty.starts_with('i')
        && (2..=64).contains(&bits)
    {
        return Some(numeric_base(NumericBaseType::IB(IBits(bits))));
    }
    None
}

fn convert_item_enum(
    current_crate: &CrateContext,
    scratch: &mut Scratch,
    ty_name: String,
    item_enum: &ItemEnum,
) -> Result<TypeOwned> {
    let mut variants = vec![];
    let mut discriminant = 0;
    for variant in &item_enum.variants {
        // holds a discriminant from a newer version, it is not a variant on the wire (SW-16)
        if variant
            .attrs
            .iter()
            .any(|a| a.path().is_ident("future_version"))
        {
            continue;
        }
        let fields = convert_fields(&variant.fields, current_crate, scratch, false)?;
        if let Some((_, explicit_discriminant)) = &variant.discriminant {
            if let Expr::Lit(expr_lit) = explicit_discriminant
                && let Lit::Int(lit_int) = &expr_lit.lit
            {
                discriminant = lit_int
                    .base10_parse()
                    .context("parsing enum discriminant")
                    .context(current_crate.err_context())?;
            } else {
                return Err(anyhow!("enum discriminant must be an integer literal")
                    .context(current_crate.err_context()));
            }
        }
        let since = get_since_attr(&variant.attrs, current_crate)?;
        variants.push(VariantOwned {
            docs: collect_docs(&variant.attrs),
            ident: variant.ident.to_string(),
            fields,
            discriminant: UNib32(discriminant),
            since,
        });
        discriminant += 1;
    }
    let repr = get_repr(&item_enum.attrs, current_crate, &ty_name)?;
    let size = get_size_assumption(&item_enum.attrs, current_crate)?;
    let ty = TypeOwned::Enum(ItemEnumOwned {
        size,
        repr,
        crate_idx: scratch.root_bundle.find_crate_or_create(current_crate),
        docs: collect_docs(&item_enum.attrs),
        ident: ty_name,
        generics: scratch.generic_scope.clone(),
        variants,
    });
    Ok(ty)
}

fn convert_item_struct(
    current_crate: &CrateContext,
    scratch: &mut Scratch,
    ty_name: String,
    item_struct: &ItemStruct,
) -> Result<TypeOwned> {
    let args = derive_shrink_wrap_args(&item_struct.attrs, current_crate)?;
    let fields = convert_fields(&item_struct.fields, current_crate, scratch, args.sparse)?;
    let ty = TypeOwned::Struct(ItemStructOwned {
        size: args.size_assumption.unwrap_or(ElementSize::Unsized),
        crate_idx: scratch.root_bundle.find_crate_or_create(current_crate),
        docs: collect_docs(&item_struct.attrs),
        ident: ty_name,
        generics: scratch.generic_scope.clone(),
        fields,
        sparse: args.sparse,
    });
    Ok(ty)
}

/// `sparse`: the struct is `#[derive_shrink_wrap(sparse, ..)]` (DER-13), so each field's [FieldOwned::slot] is its
/// `#[slot = N]`, or the previous field's slot + 1 (0 for the first), mirroring `shrink_wrap_derive`'s
/// `ItemStruct::from_syn_sparse`.
fn convert_fields(
    fields: &Fields,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
    sparse: bool,
) -> Result<FieldsOwned> {
    match fields {
        Fields::Named(_) | Fields::Unnamed(_) => {
            let mut owned = vec![];
            let mut next_slot = 0u32;
            for f in fields {
                let since = get_since_attr(&f.attrs, current_crate)?;
                let default = get_default_attr(&f.attrs, current_crate)?;
                let ty = if f.attrs.iter().any(|a| a.path().is_ident("flag")) {
                    flag_ty(f, fields, current_crate)?
                } else {
                    convert_ty(&f.ty, current_crate, scratch)?
                };
                let slot = if sparse {
                    let slot = get_slot_attr(&f.attrs, current_crate)?.unwrap_or(next_slot);
                    next_slot = slot + 1;
                    Some(UNib32(slot))
                } else {
                    None
                };
                owned.push(FieldOwned {
                    docs: collect_docs(&f.attrs),
                    ident: f.ident.as_ref().map(|i| i.to_string()),
                    ty,
                    default,
                    since,
                    slot,
                    series_opaque: has_series_opaque(&f.attrs),
                });
            }
            if matches!(fields, Fields::Unnamed(_)) {
                Ok(FieldsOwned::Unnamed(owned))
            } else {
                Ok(FieldsOwned::Named(owned))
            }
        }
        Fields::Unit => Ok(FieldsOwned::Unit),
    }
}

/// `#[series(opaque)]` (SW-60): the field is an opaque run of its bytes in a `Series`; the derive checks the form.
fn has_series_opaque(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("series")
            && matches!(&a.meta, syn::Meta::List(list) if list.tokens.to_string() == "opaque")
    })
}

/// `#[slot = N]` (DER-13, a field of a `sparse` struct).
fn get_slot_attr(attrs: &[Attribute], current_crate: &CrateContext) -> Result<Option<u32>> {
    let Some(attr) = attrs.iter().find(|a| a.path().is_ident("slot")) else {
        return Ok(None);
    };
    if let Meta::NameValue(name_value) = &attr.meta
        && let Expr::Lit(expr_lit) = &name_value.value
        && let Lit::Int(lit_int) = &expr_lit.lit
    {
        Ok(Some(
            lit_int
                .base10_parse()
                .context(current_crate.err_context())?,
        ))
    } else {
        Err(anyhow!("expected #[slot = N] with an integer literal N")
            .context(current_crate.err_context()))
    }
}

/// `#[flag] name: bool`, the relocated flag of the `Option` or `Result` field `name` that comes after it.
fn flag_ty(field: &syn::Field, fields: &Fields, current_crate: &CrateContext) -> Result<TypeOwned> {
    let err = |msg: &str| Err(anyhow!("#[flag] {msg}").context(current_crate.err_context()));
    let Some(ident) = &field.ident else {
        return err("is only supported on named fields");
    };
    if !matches!(&field.ty, Type::Path(p) if p.path.is_ident("bool")) {
        return err(&format!("{ident} must be a bool"));
    }
    let is_flagged = |f: &syn::Field| matches!(&f.ty, Type::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == "Option" || s.ident == "Result"));
    let target = fields
        .iter()
        .skip_while(|f| !std::ptr::eq(*f, field))
        .skip(1)
        .find(|f| f.ident.as_ref() == Some(ident));
    match target {
        Some(target) if is_flagged(target) => Ok(TypeOwned::Flag),
        _ => err(&format!(
            "{ident} must be followed by an Option or Result field with the same name"
        )),
    }
}

fn convert_ty_vec(
    segment: &PathSegment,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<TypeOwned> {
    let inner_ty = get_inner_angle_bracketed_ty(segment, current_crate)?;
    let inner_ty = convert_ty(inner_ty, current_crate, scratch)?;
    Ok(TypeOwned::Vec(Box::new(inner_ty)))
}

/// `MaxLen<T, N>`, `T` a list or a string, `N` an integer literal (also as `{ 64 }`) or a `const` of one in the
/// same crate's lib.rs.
fn convert_ty_max_len(
    segment: &PathSegment,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<TypeOwned> {
    let err = |msg: &str| anyhow!("MaxLen<T, N>: {msg}").context(current_crate.err_context());
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return Err(err("expected generic arguments"));
    };
    let mut args = args.args.iter();
    let (Some(GenericArgument::Type(inner_ty)), Some(len), None) =
        (args.next(), args.next(), args.next())
    else {
        return Err(err("expected a type and a length"));
    };
    let max_len = match len {
        GenericArgument::Const(expr) => convert_length(expr, current_crate, scratch),
        // `N` alone parses as a type
        GenericArgument::Type(Type::Path(type_path)) if type_path.path.get_ident().is_some() => {
            let expr = Expr::Path(syn::ExprPath {
                attrs: vec![],
                qself: None,
                path: type_path.path.clone(),
            });
            convert_length(&expr, current_crate, scratch)
        }
        _ => None,
    }
    .ok_or_else(|| {
        err("N must be an integer literal, a const of one in lib.rs or a const parameter")
    })?;
    let inner_ty = convert_ty(inner_ty, current_crate, scratch)?;
    if !matches!(inner_ty, TypeOwned::Vec(_) | TypeOwned::String) {
        return Err(err("T must be a Vec, RefVec, String or &str"));
    }
    Ok(TypeOwned::MaxLen {
        max_len,
        ty: Box::new(inner_ty),
    })
}

fn convert_ty_option(
    segment: &PathSegment,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<TypeOwned> {
    let inner_ty = get_inner_angle_bracketed_ty(segment, current_crate)?;
    let inner_ty = convert_ty(inner_ty, current_crate, scratch)?;
    Ok(TypeOwned::Option {
        some_ty: Box::new(inner_ty),
    })
}

fn convert_ty_result(
    segment: &PathSegment,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<TypeOwned> {
    let PathArguments::AngleBracketed(arg) = &segment.arguments else {
        return Err(anyhow!("expected Result<T, E>, got Result or Result()"));
    };
    let mut args = arg.args.iter();
    let (Some(ok_arg), Some(err_arg)) = (args.next(), args.next()) else {
        return Err(anyhow!("expected Result<T, E>"));
    };
    let (GenericArgument::Type(ok_ty), GenericArgument::Type(err_ty)) = (ok_arg, err_arg) else {
        return Err(anyhow!("expected Result<T, E>, got {arg:?}"));
    };
    let ok_ty = convert_ty(ok_ty, current_crate, scratch)?;
    let err_ty = convert_ty(err_ty, current_crate, scratch)?;
    Ok(TypeOwned::Result {
        ok_ty: Box::new(ok_ty),
        err_ty: Box::new(err_ty),
    })
}

fn convert_ty_range(
    segment: &PathSegment,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<TypeOwned> {
    let inner_ty = get_inner_angle_bracketed_ty(segment, current_crate)?;
    let inner_ty = convert_ty(inner_ty, current_crate, scratch)?;
    let TypeOwned::NumericAny(NumericAnyTypeOwned::Base(numeric_base)) = inner_ty else {
        return Err(anyhow!(
            "Range only supports numeric types, got {inner_ty:?}"
        ));
    };
    Ok(TypeOwned::Range(Box::new(numeric_base)))
}

fn convert_ty_range_inclusive(
    segment: &PathSegment,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<TypeOwned> {
    let inner_ty = get_inner_angle_bracketed_ty(segment, current_crate)?;
    let inner_ty = convert_ty(inner_ty, current_crate, scratch)?;
    let TypeOwned::NumericAny(NumericAnyTypeOwned::Base(numeric_base)) = inner_ty else {
        return Err(anyhow!(
            "RangeInclusive only supports numeric types, got {inner_ty:?}"
        ));
    };
    Ok(TypeOwned::RangeInclusive(Box::new(numeric_base)))
}

/// `Delta<'i, T>`, `DeltaOfDelta<'i, T>` (`T` one of `u8`..`u64`, `i8`..`i64`) and `XorFloat<'i, T>` (`f32` or `f64`),
/// also their `..Owned<T>` forms (SW-31, SW-32).
fn convert_ty_series(
    segment: &PathSegment,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<TypeOwned> {
    let name = segment.ident.to_string();
    let name = name.trim_end_matches("Owned");
    let inner_ty = get_inner_angle_bracketed_ty(segment, current_crate)?;
    let inner_ty = convert_ty(inner_ty, current_crate, scratch)?;
    use NumericBaseType::*;
    let base = match inner_ty {
        TypeOwned::NumericAny(NumericAnyTypeOwned::Base(base)) => Some(base),
        _ => None,
    };
    match (name, base) {
        ("Delta", Some(base @ (U8 | U16 | U32 | U64 | I8 | I16 | I32 | I64))) => {
            Ok(TypeOwned::Delta(base))
        }
        ("DeltaOfDelta", Some(base @ (U8 | U16 | U32 | U64 | I8 | I16 | I32 | I64))) => {
            Ok(TypeOwned::DeltaOfDelta(base))
        }
        ("XorFloat", Some(base @ (F32 | F64))) => Ok(TypeOwned::XorFloat(base)),
        ("XorFloat", _) => {
            Err(anyhow!("XorFloat<T>: T must be f32 or f64").context(current_crate.err_context()))
        }
        _ => Err(anyhow!("{name}<T>: T must be one of u8..u64, i8..i64")
            .context(current_crate.err_context())),
    }
}

fn convert_ty_ref_box(
    segment: &PathSegment,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<TypeOwned> {
    let inner_ty = get_inner_angle_bracketed_ty(segment, current_crate)?;
    let inner_ty = convert_ty(inner_ty, current_crate, scratch)?;
    Ok(TypeOwned::Box(Box::new(inner_ty)))
}

fn get_inner_angle_bracketed_ty<'i>(
    segment: &'i PathSegment,
    current_crate: &CrateContext,
) -> Result<&'i Type> {
    let outer = segment.ident.to_string(); // Vec, Option, Box, etc.
    let PathArguments::AngleBracketed(arg) = &segment.arguments else {
        return Err(
            anyhow!("{segment:?}: expected {outer}<T>, got {outer} or {outer}()")
                .context(current_crate.err_context()),
        );
    };
    let mut args = arg.args.iter();
    let Some(arg) = args.next() else {
        return Err(anyhow!("expected {outer}<T>, got {outer}<T, ?>"));
    };
    let arg = if matches!(arg, GenericArgument::Lifetime(_)) {
        let Some(arg) = args.next() else {
            return Err(anyhow!("expected {outer}<'i, T>, got {outer}<'i, T, ?>"));
        };
        arg
    } else {
        arg
    };
    let GenericArgument::Type(inner_ty) = arg else {
        return Err(anyhow!("expected {outer}<T>, got {arg:?}"));
    };
    Ok(inner_ty)
}

fn get_repr(attrs: &[Attribute], current_crate: &CrateContext, enum_name: &str) -> Result<Repr> {
    let args = derive_shrink_wrap_args(attrs, current_crate)?;
    args.ww_repr.ok_or_else(|| {
        anyhow!("ww_repr directive is required for enum: {enum_name}")
            .context(current_crate.err_context())
    })
}

fn get_size_assumption(attrs: &[Attribute], current_crate: &CrateContext) -> Result<ElementSize> {
    let args = derive_shrink_wrap_args(attrs, current_crate)?;
    Ok(args.size_assumption.unwrap_or(ElementSize::Unsized))
}

fn get_default_attr(
    attrs: &[Attribute],
    current_crate: &CrateContext,
) -> Result<Option<ValueOwned>> {
    let Some(attr) = attrs.iter().find(|a| a.path().is_ident("default")) else {
        return Ok(None);
    };
    if let Meta::NameValue(_name_value) = &attr.meta {
        // TODO: convert expression to a value, only presence of a default is used for now (in client compatibility checks)
        Ok(Some(ValueOwned::Bool(false)))
    } else {
        Err(anyhow!("expected #[default = value]").context(current_crate.err_context()))
    }
}

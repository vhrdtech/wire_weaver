use super::{
    crate_walker::{CrateContext, Scratch},
    util::{collect_docs, derive_shrink_wrap_args, get_since_attr, use_tree_path_to},
};
use anyhow::{Context, Result, anyhow};
use shrink_wrap::{ElementSize, UNib32};
use syn::{
    Attribute, Expr, Fields, GenericArgument, Item, ItemEnum, ItemStruct, Lit, Meta, PathArguments,
    PathSegment, Type, TypePath, parse_str,
};
use ww_numeric::{IBits, NumericAnyTypeOwned, UBits};
use ww_self::{
    FieldOwned, FieldsOwned, ItemEnumOwned, ItemStructOwned, NumericBaseType, Repr, TypeOwned,
    ValueOwned, VariantOwned,
};
use ww_version::FullVersionOwned;

pub(crate) fn convert_ty(
    ty: &Type,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<TypeOwned> {
    match ty {
        Type::Array(type_array) => {
            let Expr::Lit(lit) = &type_array.len else {
                return Err(anyhow!("only literals supported as array length"));
            };
            let Lit::Int(lit_int) = &lit.lit else {
                return Err(anyhow!("only integers supported as array length"));
            };
            let len: u32 = lit_int.base10_parse().context("parsing array length")?;
            let inner = convert_ty(&type_array.elem, current_crate, scratch)?;
            Ok(TypeOwned::Array {
                len: UNib32(len),
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
        "Option" => convert_ty_option(segment, current_crate, scratch),
        "Result" => convert_ty_result(segment, current_crate, scratch),
        "Range" => convert_ty_range(segment, current_crate, scratch),
        "RangeInclusive" => convert_ty_range_inclusive(segment, current_crate, scratch),
        "Box" | "RefBox" => convert_ty_ref_box(segment, current_crate, scratch),
        user_ty => {
            if let Some(ty) = convert_ub_ib(user_ty) {
                return Ok(ty);
            }

            for item in &current_crate.lib_rs_ast.items {
                match item {
                    Item::Enum(item_enum) if item_enum.ident == user_ty => {
                        let key = enter_type(current_crate, scratch, &ty_name)?;
                        let ty = convert_item_enum(current_crate, scratch, ty_name, item_enum);
                        scratch.types_in_progress.retain(|k| k != &key);
                        return ty;
                    }
                    Item::Struct(item_struct) if item_struct.ident == user_ty => {
                        let key = enter_type(current_crate, scratch, &ty_name)?;
                        let ty = convert_item_struct(current_crate, scratch, ty_name, item_struct);
                        scratch.types_in_progress.retain(|k| k != &key);
                        return ty;
                    }
                    Item::Use(item_use) => {
                        let Some(path) = use_tree_path_to(&item_use.tree, user_ty) else {
                            continue;
                        };
                        let dependent_crate = current_crate.resolve_path(path, scratch)?;
                        let ty: Type = parse_str(user_ty)?;
                        return convert_ty(&ty, &dependent_crate, scratch);
                    }
                    _ => {}
                }
            }
            Err(anyhow!("Type {segment:?} not found").context(current_crate.err_context()))
        }
    }
}

/// Mark a user-defined type as being converted, errors out if it is already, i.e., it refers to itself.
fn enter_type(
    current_crate: &CrateContext,
    scratch: &mut Scratch,
    ty_name: &str,
) -> Result<(FullVersionOwned, String)> {
    let key = (current_crate.version().clone(), ty_name.to_string());
    if scratch.types_in_progress.contains(&key) {
        return Err(
            anyhow!("Self-referential type {ty_name} is not supported yet")
                .context(current_crate.err_context()),
        );
    }
    scratch.types_in_progress.push(key.clone());
    Ok(key)
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
        let fields = convert_fields(&variant.fields, current_crate, scratch)?;
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
        variants,
    });
    if let Some(type_idx) = scratch.root_bundle.find_type(&ty) {
        return Ok(TypeOwned::OutOfLine { type_idx });
    }

    let ty = scratch.root_bundle.push_out_of_line(ty, current_crate);
    Ok(ty)
}

fn convert_item_struct(
    current_crate: &CrateContext,
    scratch: &mut Scratch,
    ty_name: String,
    item_struct: &ItemStruct,
) -> Result<TypeOwned> {
    let fields = convert_fields(&item_struct.fields, current_crate, scratch)?;
    let size = get_size_assumption(&item_struct.attrs, current_crate)?;
    let ty = TypeOwned::Struct(ItemStructOwned {
        size,
        crate_idx: scratch.root_bundle.find_crate_or_create(current_crate),
        docs: collect_docs(&item_struct.attrs),
        ident: ty_name,
        fields,
    });
    if let Some(type_idx) = scratch.root_bundle.find_type(&ty) {
        return Ok(TypeOwned::OutOfLine { type_idx });
    }

    let ty = scratch.root_bundle.push_out_of_line(ty, current_crate);
    Ok(ty)
}

fn convert_fields(
    fields: &Fields,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<FieldsOwned> {
    match fields {
        Fields::Named(_) | Fields::Unnamed(_) => {
            let mut owned = vec![];
            for f in fields {
                let since = get_since_attr(&f.attrs, current_crate)?;
                let default = get_default_attr(&f.attrs, current_crate)?;
                owned.push(FieldOwned {
                    docs: collect_docs(&f.attrs),
                    ident: f.ident.as_ref().map(|i| i.to_string()),
                    ty: convert_ty(&f.ty, current_crate, scratch)?,
                    default,
                    since,
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

fn convert_ty_vec(
    segment: &PathSegment,
    current_crate: &CrateContext,
    scratch: &mut Scratch,
) -> Result<TypeOwned> {
    let inner_ty = get_inner_angle_bracketed_ty(segment, current_crate)?;
    let inner_ty = convert_ty(inner_ty, current_crate, scratch)?;
    Ok(TypeOwned::Vec(Box::new(inner_ty)))
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

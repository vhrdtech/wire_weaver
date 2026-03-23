use super::{
    crate_walker::{CrateContext, Scratch},
    util::{collect_docs, get_since_attr, use_tree_has_type},
};
use anyhow::{anyhow, Context, Result};
use shrink_wrap::{ElementSize, UNib32};
use syn::{
    parse_str, Attribute, Expr, Fields, GenericArgument, Item, ItemEnum, ItemStruct, Lit, Meta,
    PathArguments, PathSegment, Type, TypePath, UseTree,
};
use ww_numeric::{IBits, NumericAnyTypeOwned, UBits};
use ww_self::{
    FieldOwned, FieldsOwned, ItemEnumOwned, ItemStructOwned, NumericBaseType, Repr, TypeOwned,
    ValueOwned, VariantOwned,
};

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
    let len = ty_path.path.segments.len();
    if len == 1 {
        let segment = &ty_path.path.segments[0];
        convert_ty_path_segment(segment, current_crate, scratch)
    } else if len == 2 {
        let dep_crate_name = ty_path.path.segments[0].ident.to_string();
        let dependent_crate = current_crate.load_dependent_crate(&dep_crate_name, scratch)?;
        let segment = &ty_path.path.segments[1];
        convert_ty_path_segment(segment, &dependent_crate, scratch)
    } else {
        Err(anyhow!(
            "Only support `MyType` and `ext_crate::MyType` for now"
        ))
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
                        return convert_item_enum(current_crate, scratch, ty_name, &item_enum);
                    }
                    Item::Struct(item_struct) if item_struct.ident == user_ty => {
                        return convert_item_struct(current_crate, scratch, ty_name, &item_struct);
                    }
                    Item::Use(item_use) => {
                        if !use_tree_has_type(&item_use.tree, user_ty) {
                            continue;
                        }
                        let UseTree::Path(use_path) = &item_use.tree else {
                            continue;
                        };
                        // only supporting `use ext_crate::Type` for now
                        let dep_crate_name = use_path.ident.to_string();
                        let dependent_crate =
                            current_crate.load_dependent_crate(&dep_crate_name, scratch)?;
                        let ty: Type = parse_str(&format!("{user_ty}"))?;
                        return convert_ty(&ty, &dependent_crate, scratch);
                    }
                    _ => {}
                }
            }
            Err(anyhow!("Type {segment:?} not found").context(current_crate.err_context()))
        }
    }
}

fn convert_ub_ib(user_ty: &str) -> Option<TypeOwned> {
    // u1, u2, .., u64, i2, i3, .., i64
    let user_ty = user_ty.to_lowercase();
    let Some(xn) = user_ty
        .strip_prefix("ub")
        .or_else(|| user_ty.strip_prefix("u"))
        .or_else(|| user_ty.strip_prefix("ib"))
        .or_else(|| user_ty.strip_prefix("i"))
    else {
        return None;
    };
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
    let repr = get_repr(&item_enum.attrs, current_crate, scratch, &ty_name)?;
    let size = get_size_assumption(&item_enum.attrs);
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
    let size = get_size_assumption(&item_struct.attrs);
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

fn get_repr(
    attrs: &[Attribute],
    current_crate: &CrateContext,
    scratch: &mut Scratch,
    enum_name: &str,
) -> Result<Repr> {
    let attr = attrs.iter().find(|a| a.path().is_ident("ww_repr")).ok_or(
        anyhow!("ww_repr attribute is required for enum: {enum_name}")
            .context(current_crate.err_context()),
    )?;
    let Meta::List(meta_list) = &attr.meta else {
        return Err(
            anyhow!("expected #[repr(u1..u32 / unib32 / nib)] for enum: {enum_name}")
                .context(current_crate.err_context()),
        );
    };
    let repr = meta_list.tokens.to_string();
    let repr: PathSegment = parse_str(&repr)?;
    let ty = convert_ty_path_segment(&repr, current_crate, scratch)?;
    let TypeOwned::NumericAny(NumericAnyTypeOwned::Base(base)) = ty else {
        return Err(anyhow!(
            "enum discriminant type is not a number: '{ty:?}' for enum: {enum_name}"
        )
        .context(current_crate.err_context()));
    };
    let repr = match base {
        NumericBaseType::Nibble => Repr::Nibble,
        NumericBaseType::UB(bits) => Repr::BitAligned(bits.0),
        NumericBaseType::UNib32 => Repr::UNib32,
        NumericBaseType::U8 => Repr::ByteAlignedU8,
        NumericBaseType::U16 => Repr::ByteAlignedU16,
        NumericBaseType::U32 => Repr::ByteAlignedU32,
        u => {
            return Err(
                anyhow!("unsupported enum discriminant: '{u:?}' for enum: {enum_name}")
                    .context(current_crate.err_context()),
            );
        }
    };
    Ok(repr)
}

fn get_size_assumption(attrs: &[Attribute]) -> ElementSize {
    if attrs.iter().find(|a| a.path().is_ident("sized")).is_some() {
        ElementSize::Sized { size_bits: 0 }
    } else if attrs
        .iter()
        .find(|a| a.path().is_ident("self_describing"))
        .is_some()
    {
        ElementSize::SelfDescribing
    } else if attrs
        .iter()
        .find(|a| a.path().is_ident("final_structure"))
        .is_some()
    {
        ElementSize::UnsizedFinalStructure
    } else {
        ElementSize::Unsized
    }
}

fn get_default_attr(
    attrs: &[Attribute],
    current_crate: &CrateContext,
) -> Result<Option<ValueOwned>> {
    let Some(attr) = attrs.iter().find(|a| a.path().is_ident("default")) else {
        return Ok(None);
    };
    if let Meta::NameValue(name_value) = &attr.meta
        && let Expr::Lit(expr_lit) = &name_value.value
        && let Lit::Str(_lit_str) = &expr_lit.lit
    {
        // TODO: since value
        Ok(Some(ValueOwned::Bool(false)))
    } else {
        Err(anyhow!("expected #[default = \"value literal\"]").context(current_crate.err_context()))
    }
}

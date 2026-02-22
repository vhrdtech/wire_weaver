use crate::ast::Type;
use crate::ast::path::Path;
use crate::transform::util::{FieldPath, FieldSelector};
use proc_macro2::Ident;
use syn::{Attribute, Expr, GenericArgument, Lit, PathArguments, PathSegment, ReturnType};

pub fn transform_type(
    ty: syn::Type,
    _attrs: Option<&mut Vec<Attribute>>,
    path: &FieldPath,
) -> Result<Type, String> {
    match ty {
        syn::Type::Path(type_path) => {
            if type_path.path.segments.len() == 1 {
                let path_segment = type_path.path.segments.first().unwrap();
                let ty = transform_path_segment(path_segment, path)?;
                Ok(ty)
            } else {
                let mut path = Path {
                    segments: Vec::new(),
                };
                let is_lifetime = is_lifetime(&type_path.path.segments.last().expect("").arguments);
                for segment in type_path.path.segments {
                    path.segments.push(segment.ident);
                }
                Ok(Type::External(path, is_lifetime))
            }
        }
        syn::Type::Reference(type_ref) => {
            let mut ty = transform_type(type_ref.elem.as_ref().clone(), _attrs, path)?;
            if let Type::External(_, lifetime) = &mut ty {
                *lifetime = true;
            }
            Ok(ty)
        }
        syn::Type::Tuple(type_tuple) => {
            let mut types = vec![];
            for elem in type_tuple.elems.into_iter() {
                let ty = transform_type(elem, None, path)?;
                types.push(ty);
            }
            Ok(Type::Tuple(types))
        }
        syn::Type::Array(type_array) => {
            let Expr::Lit(lit) = type_array.len else {
                return Err("only literals supported as array length".into());
            };
            let Lit::Int(lit_int) = lit.lit else {
                return Err("only integers supported as array length".into());
            };
            let len: usize = lit_int.base10_parse().unwrap();
            let ty = transform_type(*type_array.elem, None, path)?;
            Ok(Type::Array(len, Box::new(ty)))
        }
        syn::Type::Slice(type_slice) => {
            let inner = transform_type(*type_slice.elem, None, path)?;
            Ok(Type::Vec(Box::new(inner)))
        }
        u => Err(format!("{u:?} is not supported")),
    }
}

fn transform_path_segment(
    path_segment: &PathSegment,
    field_path: &FieldPath,
) -> Result<Type, String> {
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
        "Vec" | "RefVec" => transform_type_vec(path_segment, field_path)?,
        "Result" => transform_type_result(path_segment, field_path)?,
        "Option" => transform_type_option(path_segment, field_path)?,
        "Range" => transform_type_range(path_segment, field_path)?,
        "RangeInclusive" => transform_type_range_inclusive(path_segment, field_path)?,
        "RefBox" => transform_type_ref_box(path_segment, field_path)?,
        other_ty => {
            // u1, u2, .., u63, except u8, u16, ...
            if let Some(un) = other_ty
                .strip_prefix('U')
                .or_else(|| other_ty.strip_prefix('u'))
                .or_else(|| other_ty.strip_prefix('I'))
                .or_else(|| other_ty.strip_prefix('i'))
            {
                let bits: Result<u8, _> = un.parse();
                if let Ok(bits) = bits
                    && (1..=63).contains(&bits)
                {
                    return Ok(Type::External(
                        Path::new_ident(Ident::new(other_ty, path_segment.ident.span())),
                        false,
                    ));
                }
            }

            return Ok(Type::External(
                Path::new_ident(Ident::new(other_ty, path_segment.ident.span())),
                is_lifetime(&path_segment.arguments),
            ));
        }
    };
    Ok(ty)
}

fn is_lifetime(arguments: &PathArguments) -> bool {
    if let PathArguments::AngleBracketed(args) = arguments {
        let mut args = args.args.iter();
        if let Some(arg) = args.next() {
            return matches!(arg, GenericArgument::Lifetime(_));
        }
    }
    false
}

fn transform_type_result(path_segment: &PathSegment, path: &FieldPath) -> Result<Type, String> {
    let PathArguments::AngleBracketed(arg) = &path_segment.arguments else {
        return Err("expected Result<T, E>, got Result or Result()".into());
    };
    let mut args = arg.args.iter();
    let (Some(ok_arg), Some(err_arg)) = (args.next(), args.next()) else {
        return Err("expected Result<T, E>".into());
    };
    let (GenericArgument::Type(ok_ty), GenericArgument::Type(err_ty)) = (ok_arg, err_arg) else {
        return Err(format!("expected Result<T, E>, got {arg:?}"));
    };
    let ok_path = path.clone_and_push(FieldSelector::ResultIfOk);
    let ok_ty = transform_type(ok_ty.clone(), None, &ok_path)?;
    let err_path = path.clone_and_push(FieldSelector::ResultIfErr);
    let err_ty = transform_type(err_ty.clone(), None, &err_path)?;
    let flag_ident = path.flag_ident();
    Ok(Type::Result(flag_ident, Box::new((ok_ty, err_ty))))
}

fn transform_type_vec(path_segment: &PathSegment, path: &FieldPath) -> Result<Type, String> {
    let PathArguments::AngleBracketed(arg) = &path_segment.arguments else {
        return Err("expected RefVec<T>, got RefVec or RefVec()".into());
    };
    let mut args = arg.args.iter();
    let Some(arg) = args.next() else {
        return Err("expected RefVec<T>, got RefVec<T, ?>".into());
    };
    let arg = if matches!(arg, GenericArgument::Lifetime(_)) {
        let Some(arg) = args.next() else {
            return Err("expected RefVec<'i, T>, got RefVec<'i, T, ?>".into());
        };
        arg
    } else {
        arg
    };
    let GenericArgument::Type(inner_ty) = arg else {
        return Err(format!("expected RefVec<T>, got {arg:?}"));
    };
    let inner_ty = transform_type(inner_ty.clone(), None, path)?;
    Ok(Type::Vec(Box::new(inner_ty)))
}

fn transform_type_option(path_segment: &PathSegment, path: &FieldPath) -> Result<Type, String> {
    let PathArguments::AngleBracketed(arg) = &path_segment.arguments else {
        return Err("expected Option<T>, got Option or Option()".into());
    };
    let Some(arg) = arg.args.first() else {
        return Err("expected Option<T>, got Option<T, ?>".into());
    };
    let GenericArgument::Type(inner_ty) = arg else {
        return Err(format!("expected Option<T>, got {arg:?}"));
    };
    let path = path.clone_and_push(FieldSelector::OptionIsSome);
    let inner_ty = transform_type(inner_ty.clone(), None, &path)?;
    let flag_ident = path.flag_ident();
    Ok(Type::Option(flag_ident, Box::new(inner_ty)))
}

fn transform_type_range(path_segment: &PathSegment, path: &FieldPath) -> Result<Type, String> {
    let PathArguments::AngleBracketed(arg) = &path_segment.arguments else {
        return Err("expected Range<T>, got Range or Range()".into());
    };
    let Some(arg) = arg.args.first() else {
        return Err("expected Range<T>, got Range<T, ?>".into());
    };
    let GenericArgument::Type(inner_ty) = arg else {
        return Err(format!("expected Range<T>, got {arg:?}"));
    };
    let inner_ty = transform_type(inner_ty.clone(), None, path)?;
    Ok(Type::Range(Box::new(inner_ty)))
}

fn transform_type_range_inclusive(
    path_segment: &PathSegment,
    path: &FieldPath,
) -> Result<Type, String> {
    let PathArguments::AngleBracketed(arg) = &path_segment.arguments else {
        return Err("expected RangeInclusive<T>, got RangeInclusive or RangeInclusive()".into());
    };
    let Some(arg) = arg.args.first() else {
        return Err("expected RangeInclusive<T>, got RangeInclusive<T, ?>".into());
    };
    let GenericArgument::Type(inner_ty) = arg else {
        return Err(format!("expected RangeInclusive<T>, got {arg:?}"));
    };
    let inner_ty = transform_type(inner_ty.clone(), None, path)?;
    Ok(Type::RangeInclusive(Box::new(inner_ty)))
}

fn transform_type_ref_box(path_segment: &PathSegment, path: &FieldPath) -> Result<Type, String> {
    let PathArguments::AngleBracketed(arg) = &path_segment.arguments else {
        return Err("expected RefBox<'i T>, got RefBox or RefBox()".into());
    };
    let mut args = arg.args.iter();
    let Some(arg) = args.next() else {
        return Err("expected RefBox<T>, got RefBox<T, ?>".into());
    };
    let arg = if matches!(arg, GenericArgument::Lifetime(_)) {
        let Some(arg) = args.next() else {
            return Err("expected RefBox<'i, T>, got RefBox<'i, T, ?>".into());
        };
        arg
    } else {
        arg
    };
    let GenericArgument::Type(inner_ty) = arg else {
        return Err(format!("expected RefBox<T>, got {arg:?}"));
    };
    let inner_ty = transform_type(inner_ty.clone(), None, path)?;
    Ok(Type::RefBox(Box::new(inner_ty)))
}

pub fn transform_return_type(ty: ReturnType, path: &FieldPath) -> Result<Option<Type>, String> {
    match ty {
        ReturnType::Default => Ok(None),
        ReturnType::Type(_, ty) => Ok(Some(transform_type(*ty, None, path)?)),
    }
}

use anyhow::{Result, anyhow};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::{Lit, LitInt};
use ww_numeric::{NumericAnyTypeOwned, NumericBaseType};
use ww_self::{ApiBundleOwned, GenericArgOwned, TypeOwned, ValueOwned};

use crate::codegen::util::maybe_quote_cl;
use crate::transform::WIRE_WEAVER_REEXPORTS;

#[derive(Copy, Clone)]
pub(crate) enum TyPos {
    Def, // RefVec<'i, T>
    Arg, // RefVec<'_, T>
    // Return,     // RefVec<'i, T>
    Annotation, // RefVec<'_, T>
    Expr,       // Turbofish RefVec::<'_, T>
    Static,     // RefVec<'static, T>
}

pub(crate) fn ty_def(
    api_bundle: &ApiBundleOwned,
    ty: &TypeOwned,
    alloc: bool,
    ty_pos: TyPos,
) -> Result<TokenStream> {
    ty_def_inner(api_bundle, ty, alloc, ty_pos, None)
}

pub(crate) fn ty_def_by_idx(
    api_bundle: &ApiBundleOwned,
    type_idx: u32,
    alloc: bool,
    ty_pos: TyPos,
) -> Result<TokenStream> {
    let ty = api_bundle.get_ty(type_idx)?;
    ty_def_inner(api_bundle, ty.0, alloc, ty_pos, Some(ty.1))
}

fn ty_def_inner(
    api_bundle: &ApiBundleOwned,
    ty: &TypeOwned,
    alloc: bool,
    ty_pos: TyPos,
    crate_idx: Option<u32>,
) -> Result<TokenStream> {
    let maybe_turbofish = maybe_quote_cl(matches!(ty_pos, TyPos::Expr), || quote! { :: });
    match ty {
        TypeOwned::Bool => Ok(quote! { bool }),
        TypeOwned::NumericAny(numeric_any) => Ok(ty_def_numeric_any(numeric_any)),
        TypeOwned::OutOfLine { type_idx } => {
            let (ty, crate_idx) = api_bundle.get_ty(type_idx.0)?;
            ty_def_inner(api_bundle, ty, alloc, ty_pos, Some(crate_idx))
        }
        // `Timed<Ampere>`, `TaggedOwned<String>` (API-43): the definition's own lifetime first, then the arguments
        TypeOwned::Instance { type_idx, args } => {
            let (def, crate_idx) = api_bundle.get_ty(type_idx.0)?;
            let (ident, own_lifetime) = match def {
                TypeOwned::Struct(item_struct) => {
                    (&item_struct.ident, item_struct.is_lifetime(api_bundle)?)
                }
                TypeOwned::Enum(item_enum) => (&item_enum.ident, item_enum.is_lifetime(api_bundle)?),
                _ => return Err(anyhow!("generic type with index {} is not a struct or an enum", type_idx.0)),
            };
            let mut generic_args = vec![];
            if own_lifetime && !alloc {
                generic_args.push(lifetime(ty_pos));
            }
            for arg in args {
                generic_args.push(match arg {
                    GenericArgOwned::Type(ty) => ty_def_inner(api_bundle, ty, alloc, ty_pos, None)?,
                    GenericArgOwned::Const(value) => const_arg(value)?,
                    GenericArgOwned::ConstParam(_) => {
                        return Err(anyhow!("const parameter of {ident} outside of a generic definition"));
                    }
                });
            }
            let path = user_ty_path(Some(crate_idx), ident, own_lifetime && alloc, api_bundle)?;
            Ok(quote! { #path #maybe_turbofish < #(#generic_args),* > })
        }
        TypeOwned::Param(idx) => Err(anyhow!(
            "type parameter {} outside of a generic definition",
            idx.0
        )),
        TypeOwned::Flag => Err(anyhow!("Flag type cannot be in def position")),
        TypeOwned::String => Ok(if alloc {
            quote! { String }
        } else {
            let l = lifetime(ty_pos);
            quote! { & #l str }
        }),
        TypeOwned::Vec(inner_ty) => {
            let inner_ty = ty_def_inner(api_bundle, inner_ty, alloc, ty_pos, None)?;
            if alloc {
                Ok(quote! { Vec<#inner_ty> })
            } else {
                let l = lifetime(ty_pos);
                Ok(quote! { wire_weaver::shrink_wrap::RefVec #maybe_turbofish <#l, #inner_ty> })
            }
        }
        TypeOwned::Array { len, ty } => {
            let ty = ty_def_inner(api_bundle, ty, alloc, ty_pos, None)?;
            let len = Lit::Int(LitInt::new(
                format!("{}", len.fixed()?).as_str(),
                Span::call_site(),
            ));
            Ok(quote! { [ #ty; #len ] })
        }
        TypeOwned::Tuple(types) => {
            let types: Result<Vec<TokenStream>, _> = types
                .iter()
                .map(|ty| ty_def_inner(api_bundle, ty, alloc, ty_pos, None))
                .collect();
            let types = types?;
            Ok(quote! { ( #(#types),* ) })
        }
        TypeOwned::Struct(item_struct) => user_ty_def(
            crate_idx,
            &item_struct.ident,
            item_struct.is_lifetime(api_bundle)?,
            alloc,
            ty_pos,
            api_bundle,
        ),
        TypeOwned::Enum(item_enum) => user_ty_def(
            crate_idx,
            &item_enum.ident,
            item_enum.is_lifetime(api_bundle)?,
            alloc,
            ty_pos,
            api_bundle,
        ),
        TypeOwned::Option { some_ty } => {
            let some_ty = ty_def_inner(api_bundle, some_ty, alloc, ty_pos, None)?;
            Ok(quote! { Option #maybe_turbofish <#some_ty> })
        }
        TypeOwned::Result { ok_ty, err_ty } => {
            let ok_ty = ty_def_inner(api_bundle, ok_ty, alloc, ty_pos, None)?;
            let err_ty = ty_def_inner(api_bundle, err_ty, alloc, ty_pos, None)?;
            Ok(quote! { Result #maybe_turbofish <#ok_ty, #err_ty> })
        }
        TypeOwned::Box(inner_ty) => {
            let inner_ty = ty_def_inner(api_bundle, inner_ty, alloc, ty_pos, None)?;
            Ok(if alloc {
                quote! { Box #maybe_turbofish <#inner_ty> }
            } else {
                let l = lifetime(ty_pos);
                quote! { wire_weaver::shrink_wrap::RefBox #maybe_turbofish <#l, #inner_ty> }
            })
        }
        TypeOwned::Range(numeric_base) => {
            let numeric_base = ty_def_numeric_base(numeric_base);
            Ok(quote! { core::ops::Range #maybe_turbofish <#numeric_base> })
        }
        TypeOwned::RangeInclusive(numeric_base) => {
            let numeric_base = ty_def_numeric_base(numeric_base);
            Ok(quote! { core::ops::RangeInclusive #maybe_turbofish <#numeric_base> })
        }
        TypeOwned::Delta(base) | TypeOwned::DeltaOfDelta(base) | TypeOwned::XorFloat(base) => {
            let base = ty_def_numeric_base(base);
            let name = match (ty, alloc) {
                (TypeOwned::Delta(_), false) => "Delta",
                (TypeOwned::Delta(_), true) => "DeltaOwned",
                (TypeOwned::DeltaOfDelta(_), false) => "DeltaOfDelta",
                (TypeOwned::DeltaOfDelta(_), true) => "DeltaOfDeltaOwned",
                (_, false) => "XorFloat",
                (_, true) => "XorFloatOwned",
            };
            let name = Ident::new(name, Span::call_site());
            Ok(if alloc {
                quote! { wire_weaver::shrink_wrap::#name #maybe_turbofish <#base> }
            } else {
                let l = lifetime(ty_pos);
                quote! { wire_weaver::shrink_wrap::#name #maybe_turbofish <#l, #base> }
            })
        }
        TypeOwned::MaxLen { max_len, ty } => {
            let inner_ty = ty_def_inner(api_bundle, ty, alloc, ty_pos, None)?;
            let max_len = Lit::Int(LitInt::new(
                format!("{}", max_len.fixed()?).as_str(),
                Span::call_site(),
            ));
            Ok(quote! { wire_weaver::shrink_wrap::MaxLen #maybe_turbofish <#inner_ty, #max_len> })
        }
    }
}

/// A const argument as a literal: `64`, `-1`, `true`.
fn const_arg(value: &ValueOwned) -> Result<TokenStream> {
    match value {
        ValueOwned::Bool(b) => Ok(quote! { #b }),
        ValueOwned::Numeric(n) => {
            let v = n
                .to_i128()
                .ok_or_else(|| anyhow!("const argument {n:?} is not an integer"))?;
            let lit = LitInt::new(&v.unsigned_abs().to_string(), Span::call_site());
            Ok(if v < 0 {
                quote! { { -#lit } }
            } else {
                quote! { #lit }
            })
        }
        other => Err(anyhow!("const argument {other:?} is not a bool or an integer")),
    }
}

/// `crate::Name`, `crate::NameOwned` with `owned`.
fn user_ty_path(
    crate_idx: Option<u32>,
    ty_name: &str,
    owned: bool,
    api_bundle: &ApiBundleOwned,
) -> Result<TokenStream> {
    let source_crate = source_crate(crate_idx, api_bundle)?;
    let ty_name = if owned {
        Ident::new(&format!("{ty_name}Owned"), Span::call_site())
    } else {
        Ident::new(ty_name, Span::call_site())
    };
    Ok(quote! { #source_crate::#ty_name })
}

fn source_crate(crate_idx: Option<u32>, api_bundle: &ApiBundleOwned) -> Result<TokenStream> {
    Ok(if let Some(crate_idx) = crate_idx {
        let name = api_bundle.crate_name(crate_idx)?;
        let reexported = WIRE_WEAVER_REEXPORTS.contains(&name);
        let name = Ident::new(name, Span::call_site());
        if reexported {
            // `CommonError` without a dependency on `ww_error`, as the API crate itself
            quote! { wire_weaver::#name }
        } else {
            quote! { #name }
        }
    } else {
        quote! {}
    })
}

fn user_ty_def(
    crate_idx: Option<u32>,
    ty_name: &str,
    is_lifetime: bool,
    alloc: bool,
    ty_pos: TyPos,
    api_bundle: &ApiBundleOwned,
) -> Result<TokenStream> {
    let source_crate = source_crate(crate_idx, api_bundle)?;
    let maybe_turbofish = maybe_quote_cl(matches!(ty_pos, TyPos::Expr), || quote! { :: });

    if alloc {
        let ty_name = if is_lifetime {
            Ident::new(&format!("{ty_name}Owned"), Span::call_site())
        } else {
            Ident::new(ty_name, Span::call_site())
        };
        Ok(quote! { #source_crate::#ty_name })
    } else {
        // no_std
        let ty_name = Ident::new(ty_name, Span::call_site());
        if is_lifetime {
            let l = lifetime(ty_pos);
            Ok(quote! { #source_crate::#ty_name #maybe_turbofish <#l> })
        } else {
            Ok(quote! { #source_crate::#ty_name })
        }
    }
}

fn ty_def_numeric_any(numeric_any: &NumericAnyTypeOwned) -> TokenStream {
    match numeric_any {
        NumericAnyTypeOwned::Base(base) => ty_def_numeric_base(base),
        NumericAnyTypeOwned::SubType { .. } => todo!(),
        NumericAnyTypeOwned::ShiftScale { .. } => todo!(),
    }
}

fn ty_def_numeric_base(base: &NumericBaseType) -> TokenStream {
    match base {
        NumericBaseType::Nibble => quote! { wire_weaver::shrink_wrap::Nibble },
        NumericBaseType::U8 => quote! { u8 },
        NumericBaseType::U16 => quote! { u16 },
        NumericBaseType::U32 => quote! { u32 },
        NumericBaseType::UNib32 => quote! { wire_weaver::shrink_wrap::UNib32 },
        NumericBaseType::U64 => quote! { u64 },
        NumericBaseType::I32 => quote! { i32 },
        NumericBaseType::F32 => quote! { f32 },
        NumericBaseType::U128 => quote! { u128 },
        NumericBaseType::I8 => quote! { i8 },
        NumericBaseType::I16 => quote! { i16 },
        NumericBaseType::I64 => quote! { i64 },
        NumericBaseType::I128 => quote! { i128 },
        NumericBaseType::F16 => todo!(),
        NumericBaseType::F64 => quote! { f64 },
        // bit-aligned `U1`..`U64` / `I2`..`I64`, `U8` is not `u8` (SW-48)
        NumericBaseType::UB(bits) => {
            let ty = Ident::new(&format!("U{}", bits.0), Span::call_site());
            quote! { wire_weaver::shrink_wrap::un::#ty }
        }
        NumericBaseType::IB(bits) => {
            let ty = Ident::new(&format!("I{}", bits.0), Span::call_site());
            quote! { wire_weaver::shrink_wrap::un::#ty }
        }
        NumericBaseType::UN => todo!(),
        NumericBaseType::IN => todo!(),
        NumericBaseType::ULeb32 => todo!(),
        NumericBaseType::ULeb64 => todo!(),
        NumericBaseType::ULeb128 => todo!(),
        NumericBaseType::ILeb32 => todo!(),
        NumericBaseType::ILeb64 => todo!(),
        NumericBaseType::ILeb128 => todo!(),
        NumericBaseType::UQ { .. } => todo!(),
        NumericBaseType::IQ { .. } => todo!(),
        NumericBaseType::UVlq32 => quote! { wire_weaver::shrink_wrap::UVlq32 },
        NumericBaseType::UVlq64 => quote! { wire_weaver::shrink_wrap::UVlq64 },
    }
}

fn lifetime(ty_pos: TyPos) -> TokenStream {
    match ty_pos {
        TyPos::Def /*| TyPos::Return*/ => quote! { 'i },
        TyPos::Arg | TyPos::Expr => quote! { '_ },
        TyPos::Annotation => quote! { '_ },
        TyPos::Static => quote! { 'static },
    }
}

use anyhow::{anyhow, Result};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::{Lit, LitInt};
use ww_numeric::{NumericAnyTypeOwned, NumericBaseType};
use ww_self::{ApiBundleOwned, TypeOwned};

pub(crate) fn ty_def(
    api_bundle: &ApiBundleOwned,
    ty: &TypeOwned,
    alloc: bool,
    arg_pos: bool,
) -> Result<TokenStream> {
    ty_def_inner(api_bundle, ty, alloc, arg_pos, None)
}

fn ty_def_inner(
    api_bundle: &ApiBundleOwned,
    ty: &TypeOwned,
    alloc: bool,
    arg_pos: bool,
    crate_idx: Option<u32>,
) -> Result<TokenStream> {
    match ty {
        TypeOwned::Bool => Ok(quote! { bool }),
        TypeOwned::NumericAny(numeric_any) => Ok(ty_def_numeric_any(numeric_any)),
        TypeOwned::OutOfLine { type_idx } => {
            let (ty, crate_idx) = api_bundle.get_ty(type_idx.0)?;
            ty_def_inner(api_bundle, ty, alloc, arg_pos, Some(crate_idx))
        }
        TypeOwned::Flag => Err(anyhow!("Flag type cannot be in def position")),
        TypeOwned::String => Ok(if alloc {
            quote! { String }
        } else {
            let l = lifetime(arg_pos);
            quote! { & #l str }
        }),
        TypeOwned::Vec(inner_ty) => {
            let inner_ty = ty_def_inner(api_bundle, inner_ty, alloc, arg_pos, None)?;
            if alloc {
                Ok(quote! { Vec<#inner_ty> })
            } else {
                let l = lifetime(arg_pos);
                Ok(quote! { shrink_wrap::RefVec<#l, #inner_ty> })
            }
        }
        TypeOwned::Array { len, ty } => {
            let ty = ty_def_inner(api_bundle, ty, alloc, arg_pos, None)?;
            let len = Lit::Int(LitInt::new(
                format!("{}", len.0).as_str(),
                Span::call_site(),
            ));
            Ok(quote! { [ #len; #ty ] })
        }
        TypeOwned::Tuple(types) => {
            let types: Result<Vec<TokenStream>, _> = types
                .iter()
                .map(|ty| ty_def_inner(api_bundle, ty, alloc, arg_pos, None))
                .collect();
            let types = types?;
            Ok(quote! { ( #(#types),* ) })
        }
        TypeOwned::Struct(item_struct) => user_ty_def(
            crate_idx,
            &item_struct.ident,
            item_struct.is_lifetime(api_bundle)?,
            alloc,
            arg_pos,
            api_bundle,
        ),
        TypeOwned::Enum(item_enum) => user_ty_def(
            crate_idx,
            &item_enum.ident,
            item_enum.is_lifetime(api_bundle)?,
            alloc,
            arg_pos,
            api_bundle,
        ),
        TypeOwned::Option { some_ty } => {
            let some_ty = ty_def_inner(api_bundle, some_ty, alloc, arg_pos, None)?;
            Ok(quote! { Option<#some_ty> })
        }
        TypeOwned::Result { ok_ty, err_ty } => {
            let ok_ty = ty_def_inner(api_bundle, ok_ty, alloc, arg_pos, None)?;
            let err_ty = ty_def_inner(api_bundle, err_ty, alloc, arg_pos, None)?;
            Ok(quote! { Result<#ok_ty, #err_ty> })
        }
        TypeOwned::Box(inner_ty) => {
            let inner_ty = ty_def_inner(api_bundle, inner_ty, alloc, arg_pos, None)?;
            Ok(if alloc {
                quote! { Box<#inner_ty> }
            } else {
                let l = lifetime(arg_pos);
                quote! { shrink_wrap::RefBox<#l, #inner_ty> }
            })
        }
        TypeOwned::Range(numeric_base) => {
            let numeric_base = ty_def_numeric_base(numeric_base);
            Ok(quote! { core::ops::Range<#numeric_base> })
        }
        TypeOwned::RangeInclusive(numeric_base) => {
            let numeric_base = ty_def_numeric_base(numeric_base);
            Ok(quote! { core::ops::RangeInclusive<#numeric_base> })
        }
    }
}

fn user_ty_def(
    crate_idx: Option<u32>,
    ty_name: &str,
    is_unsized: bool,
    alloc: bool,
    arg_pos: bool,
    api_bundle: &ApiBundleOwned,
) -> Result<TokenStream> {
    let source_crate = if let Some(crate_idx) = crate_idx {
        let name = api_bundle.crate_name(crate_idx)?;
        let name = Ident::new(name, Span::call_site());
        quote! { #name }
    } else {
        quote! {}
    };

    if alloc {
        let ty_name = if is_unsized {
            Ident::new(&format!("{ty_name}Owned"), Span::call_site())
        } else {
            Ident::new(ty_name, Span::call_site())
        };
        Ok(quote! { #source_crate::#ty_name })
    } else {
        // no_std
        let ty_name = Ident::new(ty_name, Span::call_site());
        if is_unsized {
            let l = lifetime(arg_pos);
            Ok(quote! { #source_crate::#ty_name<#l> })
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
        NumericBaseType::Nibble => quote! { shrink_wrap::Nibble },
        NumericBaseType::U8 => quote! { u8 },
        NumericBaseType::U16 => quote! { u16 },
        NumericBaseType::U32 => quote! { u32 },
        NumericBaseType::UNib32 => quote! { shrink_wrap::UNib32 },
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
        NumericBaseType::UB(_) => todo!(),
        NumericBaseType::IB(_) => todo!(),
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
    }
}

fn lifetime(arg_pos: bool) -> TokenStream {
    if arg_pos {
        quote! { '_ }
    } else {
        quote! { 'i }
    }
}

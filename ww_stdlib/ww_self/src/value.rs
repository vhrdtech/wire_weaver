use crate::{ApiBundleOwned, FieldsOwned, FieldsValueOwned, Repr, TypeOwned, ValueOwned};
use anyhow::{anyhow, Result};
use shrink_wrap::{BufReader, Nibble};
use ww_numeric::{NumericAnyTypeOwned, NumericBaseType, NumericValue};

impl ValueOwned {
    pub fn from_shrink_wrap(
        bytes: &[u8],
        ty: &TypeOwned,
        api_bundle: &ApiBundleOwned,
    ) -> Result<Self> {
        let mut rd = BufReader::new(bytes);
        from_shrink_wrap_inner(&mut rd, ty, api_bundle)
    }
}

fn read(rd: &mut BufReader, ty: &TypeOwned, api_bundle: &ApiBundleOwned) -> Result<ValueOwned> {
    if ty.is_unsized(api_bundle)? {
        let len = rd.read_unib32_rev()?;
        let mut rd = rd.split(len as usize)?;
        from_shrink_wrap_inner(&mut rd, ty, api_bundle)
    } else {
        from_shrink_wrap_inner(rd, ty, api_bundle)
    }
}

fn from_shrink_wrap_inner(
    rd: &mut BufReader,
    ty: &TypeOwned,
    api_bundle: &ApiBundleOwned,
) -> Result<ValueOwned> {
    match ty {
        TypeOwned::Bool => Ok(ValueOwned::Bool(rd.read_bool()?)),
        TypeOwned::NumericAny(numeric_any) => {
            Ok(ValueOwned::Numeric(from_numeric_any(rd, numeric_any)?))
        }
        TypeOwned::OutOfLine { type_idx } => {
            let ty = api_bundle.get_ty(type_idx.0)?.0;
            from_shrink_wrap_inner(rd, ty, api_bundle)
        }
        // TypeOwned::Flag => {}
        TypeOwned::String => Ok(ValueOwned::String(rd.read_raw_str()?.to_string())),
        TypeOwned::Vec(inner_ty) => {
            let len = rd.read_unib32_rev()?;
            let mut items = vec![];
            for _ in 0..len {
                let value = read(rd, inner_ty, api_bundle)?;
                items.push(value);
            }
            Ok(ValueOwned::Vec(items))
        }
        TypeOwned::Array { len, ty } => {
            let mut items = vec![];
            for _ in 0..len.0 {
                let value = read(rd, ty, api_bundle)?;
                items.push(value);
            }
            Ok(ValueOwned::Array(items))
        }
        TypeOwned::Tuple(types) => {
            let mut values = vec![];
            for ty in types {
                let value = read(rd, ty, api_bundle)?;
                values.push(value);
            }
            Ok(ValueOwned::Tuple(values))
        }
        TypeOwned::Struct(struct_def) => {
            let fields = process_fields(rd, &struct_def.fields, api_bundle)?;
            Ok(ValueOwned::Struct {
                ident: struct_def.ident.clone(),
                fields,
            })
        }
        TypeOwned::Enum(enum_def) => {
            let discriminant = match enum_def.repr {
                Repr::Nibble => rd.read_nib_value()? as u32,
                Repr::BitAligned(bits) => rd.read_un32(bits)?,
                Repr::UNib32 => rd.read_unib32()?,
                Repr::ByteAlignedU8 => rd.read_u8()? as u32,
                Repr::ByteAlignedU16 => rd.read_u16()? as u32,
                Repr::ByteAlignedU32 => rd.read_u32()?,
            };
            let Some(variant) = enum_def.variants.get(discriminant as usize) else {
                return Err(anyhow!(
                    "Enum '{}' does not have variant: {}",
                    enum_def.ident,
                    discriminant
                ));
            };
            let fields = process_fields(rd, &variant.fields, api_bundle)?;

            Ok(ValueOwned::Enum {
                ident: enum_def.ident.to_string(),
                variant: (variant.ident.to_string(), fields),
            })
        }
        TypeOwned::Option { some_ty } => {
            let is_some = rd.read_bool()?;
            if is_some {
                let value = read(rd, some_ty, api_bundle)?;
                Ok(ValueOwned::Option(Some(Box::new(value))))
            } else {
                Ok(ValueOwned::Option(None))
            }
        }
        TypeOwned::Result { ok_ty, err_ty } => {
            let is_ok = rd.read_bool()?;
            if is_ok {
                let value = read(rd, ok_ty, api_bundle)?;
                Ok(ValueOwned::Result(Ok(Box::new(value))))
            } else {
                let value = read(rd, err_ty, api_bundle)?;
                Ok(ValueOwned::Result(Err(Box::new(value))))
            }
        }
        // TypeOwned::Box(_) => {}
        // TypeOwned::Range(_) => {}
        // TypeOwned::RangeInclusive(_) => {}
        u => Err(anyhow::anyhow!("Unsupported type: {:?}", u)),
    }
}

fn process_fields(
    rd: &mut BufReader,
    fields: &FieldsOwned,
    api_bundle: &ApiBundleOwned,
) -> Result<FieldsValueOwned> {
    let fields = match fields {
        FieldsOwned::Named(fields_def) | FieldsOwned::Unnamed(fields_def) => {
            let mut fields_named = vec![];
            let mut fields_unnamed = vec![];
            for field_def in fields_def {
                let value = read(rd, &field_def.ty, api_bundle)?;
                if let Some(name) = &field_def.ident {
                    fields_named.push((name.clone(), value));
                } else {
                    fields_unnamed.push(value);
                }
            }
            match fields {
                FieldsOwned::Named(_) => FieldsValueOwned::Named(fields_named),
                FieldsOwned::Unnamed(_) => FieldsValueOwned::Unnamed(fields_unnamed),
                _ => unreachable!(),
            }
        }
        FieldsOwned::Unit => FieldsValueOwned::Unit,
    };
    Ok(fields)
}

fn from_numeric_any(rd: &mut BufReader, numeric_ty: &NumericAnyTypeOwned) -> Result<NumericValue> {
    match numeric_ty {
        NumericAnyTypeOwned::Base(base_ty) => match base_ty {
            NumericBaseType::Nibble => {
                let nib: Nibble = rd.read()?;
                Ok(NumericValue::Nibble(nib))
            }
            NumericBaseType::U8 => Ok(NumericValue::U8(rd.read_u8()?)),
            NumericBaseType::U16 => Ok(NumericValue::U16(rd.read_u16()?)),
            NumericBaseType::U32 => Ok(NumericValue::U32(rd.read_u32()?)),
            NumericBaseType::UNib32 => Ok(NumericValue::UNib32(rd.read_unib32()?)),
            NumericBaseType::U64 => Ok(NumericValue::U64(rd.read_u64()?)),
            NumericBaseType::I32 => Ok(NumericValue::I32(rd.read_i32()?)),
            NumericBaseType::F32 => Ok(NumericValue::F32(rd.read_f32()?)),
            NumericBaseType::U128 => Ok(NumericValue::U128(rd.read_u128()?)),
            NumericBaseType::I8 => Ok(NumericValue::I8(rd.read_i8()?)),
            NumericBaseType::I16 => Ok(NumericValue::I16(rd.read_i16()?)),
            NumericBaseType::I64 => Ok(NumericValue::I64(rd.read_i64()?)),
            NumericBaseType::I128 => Ok(NumericValue::I128(rd.read_i128()?)),
            // NumericBaseType::F16 => Ok(NumericValue::F16(rd.read_u8()?)),
            NumericBaseType::F64 => Ok(NumericValue::F64(rd.read_f64()?)),
            // NumericBaseType::UB(bits) => Ok(NumericValue::UB)
            // NumericBaseType::IB(bits) => {}
            // NumericBaseType::UN => {}
            // NumericBaseType::IN => {}
            // NumericBaseType::ULeb32 => {}
            // NumericBaseType::ULeb64 => {}
            // NumericBaseType::ULeb128 => {}
            // NumericBaseType::ILeb32 => {}
            // NumericBaseType::ILeb64 => {}
            // NumericBaseType::ILeb128 => {}
            // NumericBaseType::UQ { .. } => {}
            // NumericBaseType::IQ { .. } => {}
            u => Err(anyhow::anyhow!("Unsupported numeric base type: {:?}", u)),
        },
        // NumericAnyTypeOwned::SubType { .. } => {}
        // NumericAnyTypeOwned::ShiftScale { .. } => {}
        u => Err(anyhow::anyhow!("Unsupported numeric type: {:?}", u)),
    }
}

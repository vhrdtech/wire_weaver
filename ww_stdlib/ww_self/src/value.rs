use crate::{ApiBundleOwned, FieldsOwned, FieldsValueOwned, Repr, TypeOwned, ValueOwned};
use anyhow::{anyhow, Result};
use shrink_wrap::{BufReader, BufWriter, Nibble};
use ww_numeric::{NumericAnyTypeOwned, NumericBaseType, NumericValue};

impl ValueOwned {
    pub fn des_shrink_wrap_dyn(
        bytes: &[u8],
        ty: &TypeOwned,
        api_bundle: &ApiBundleOwned,
    ) -> Result<Self> {
        let mut rd = BufReader::new(bytes);
        from_shrink_wrap_inner(&mut rd, ty, api_bundle)
    }

    pub fn ser_shrink_wrap_dyn(
        &self,
        ty: &TypeOwned,
        api_bundle: &ApiBundleOwned,
    ) -> Result<Vec<u8>> {
        let mut buf = [0u8; 8192]; // TODO: Use Vec flavor
        let mut wr = BufWriter::new(&mut buf);
        to_shrink_wrap_inner(&mut wr, self, ty, api_bundle)?;
        Ok(wr.finish_and_take()?.to_vec())
    }

    pub fn ser_shrink_wrap_vec_dyn<'i>(
        values: &[ValueOwned],
        mut types: impl Iterator<Item = &'i TypeOwned>,
        api_bundle: &ApiBundleOwned,
    ) -> Result<Vec<u8>> {
        let mut buf = [0u8; 8192]; // TODO: Use Vec flavor
        let mut wr = BufWriter::new(&mut buf);
        for value in values.iter() {
            let ty = types
                .next()
                .ok_or(anyhow!("values and types must have the same length"))?;
            to_shrink_wrap_inner(&mut wr, value, ty, api_bundle)?;
        }
        Ok(wr.finish_and_take()?.to_vec())
    }

    pub fn default(ty: &TypeOwned, api_bundle: &ApiBundleOwned) -> Result<Self> {
        match ty {
            TypeOwned::Bool => Ok(ValueOwned::Bool(false)),
            TypeOwned::NumericAny(any) => Ok(ValueOwned::Numeric(any.default())),
            TypeOwned::OutOfLine { type_idx } => {
                let ty = api_bundle.get_ty(type_idx.0)?;
                Self::default(ty.0, api_bundle)
            }
            TypeOwned::Flag => Err(anyhow!("flag type cannot be created manually")),
            TypeOwned::String => Ok(ValueOwned::String(String::new())),
            TypeOwned::Vec(_) => Ok(ValueOwned::Vec(vec![])),
            TypeOwned::Array { len, ty } => {
                Ok(ValueOwned::Array(vec![
                    ValueOwned::default(ty, api_bundle)?;
                    len.0 as usize
                ]))
            }
            TypeOwned::Tuple(types) => {
                let mut values = vec![];
                for ty in types {
                    values.push(Self::default(ty, api_bundle)?);
                }
                Ok(ValueOwned::Tuple(values))
            }
            TypeOwned::Struct(item_struct) => Ok(ValueOwned::Struct {
                fields: Self::default_fields_value(&item_struct.fields, api_bundle)?,
            }),
            TypeOwned::Enum(item_enum) => {
                let variant = item_enum.variants.first().ok_or(anyhow!("Enum is empty"))?;
                let fields = Self::default_fields_value(&variant.fields, api_bundle)?;
                Ok(ValueOwned::Enum {
                    variant: variant.ident.clone(),
                    fields,
                })
            }
            TypeOwned::Option { .. } => Ok(ValueOwned::Option(None)),
            TypeOwned::Result { ok_ty, .. } => Ok(ValueOwned::Result(Err(Box::new(
                Self::default(ok_ty, api_bundle)?,
            )))),
            TypeOwned::Box(inner) => Ok(Self::default(inner, api_bundle)?),
            TypeOwned::Range(base) => Ok(ValueOwned::Range(base.default()..base.default())),
            TypeOwned::RangeInclusive(base) => {
                Ok(ValueOwned::RangeInclusive(base.default()..=base.default()))
            }
        }
    }

    pub fn default_fields_value(
        fields: &FieldsOwned,
        api_bundle: &ApiBundleOwned,
    ) -> Result<FieldsValueOwned> {
        match fields {
            FieldsOwned::Named(named) => {
                let mut named_values = vec![];
                for field in named {
                    let name = field.ident.clone().unwrap_or_default();
                    let value = ValueOwned::default(&field.ty, api_bundle)?;
                    named_values.push((name, value));
                }
                Ok(FieldsValueOwned::Named(named_values))
            }
            FieldsOwned::Unnamed(unnamed) => {
                let mut unnamed_values = vec![];
                for field in unnamed {
                    let value = ValueOwned::default(&field.ty, api_bundle)?;
                    unnamed_values.push(value);
                }
                Ok(FieldsValueOwned::Unnamed(unnamed_values))
            }
            FieldsOwned::Unit => Ok(FieldsValueOwned::Unit),
        }
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
            Ok(ValueOwned::Struct { fields })
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
                variant: variant.ident.to_string(),
                fields,
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

fn to_shrink_wrap_inner(
    wr: &mut BufWriter,
    value: &ValueOwned,
    ty: &TypeOwned,
    api_bundle: &ApiBundleOwned,
) -> Result<()> {
    let ty = ty.get_in_line(api_bundle)?;
    match value {
        ValueOwned::Bool(value) => {
            wr.write_bool(*value)?;
        }
        ValueOwned::Numeric(_) => {}
        ValueOwned::String(_) => {}
        ValueOwned::Vec(_) => {}
        ValueOwned::Array(_) => {}
        ValueOwned::Tuple(_) => {}
        ValueOwned::Struct { .. } => {}
        ValueOwned::Enum { variant, fields } => {
            let TypeOwned::Enum(item_enum) = ty else {
                return Err(anyhow::anyhow!("Enum type expected"));
            };
            let discriminant = item_enum.discriminant(variant.as_str())?;
            match item_enum.repr {
                Repr::Nibble => {
                    let nib = if discriminant <= 15 {
                        Nibble::new_masked(discriminant as u8)
                    } else {
                        return Err(anyhow::anyhow!(
                            "Enum discriminant out of nibble range: {}",
                            discriminant
                        ));
                    };
                    wr.write_nib(nib)?;
                }
                Repr::BitAligned(bits) => {
                    let max_value = 2u32.pow(bits as u32) - 1;
                    if discriminant > max_value {
                        return Err(anyhow::anyhow!(
                            "Enum discriminant out of u{bits} range: {}",
                            discriminant
                        ));
                    }
                    wr.write_un32(bits, discriminant)?;
                }
                Repr::UNib32 => {
                    wr.write_unib32(discriminant)?;
                }
                Repr::ByteAlignedU8 | Repr::ByteAlignedU16 | Repr::ByteAlignedU32 => {
                    let max_value = match item_enum.repr {
                        Repr::ByteAlignedU8 => u8::MAX as u32,
                        Repr::ByteAlignedU16 => u16::MAX as u32,
                        Repr::ByteAlignedU32 => u32::MAX,
                        _ => unreachable!(),
                    };
                    if discriminant > max_value {
                        return Err(anyhow::anyhow!(
                            "Enum discriminant out range (0..={max_value}): {}",
                            discriminant
                        ));
                    }
                    match item_enum.repr {
                        Repr::ByteAlignedU8 => wr.write_u8(discriminant as u8)?,
                        Repr::ByteAlignedU16 => wr.write_u16(discriminant as u16)?,
                        Repr::ByteAlignedU32 => wr.write_u32(discriminant)?,
                        _ => unreachable!(),
                    }
                }
            }
        }
        ValueOwned::Option(value) => {
            let TypeOwned::Option { some_ty } = ty else {
                return Err(anyhow::anyhow!("Option type expected"));
            };
            wr.write_bool(value.is_some())?;
            if let Some(value) = value {
                to_shrink_wrap_inner(wr, value, some_ty, api_bundle)?;
            }
        }
        ValueOwned::Result(_) => {}
        ValueOwned::Range(_) => {}
        ValueOwned::RangeInclusive(_) => {}
    }
    Ok(())
}

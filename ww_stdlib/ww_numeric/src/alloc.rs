use crate::{NumericAnyTypeOwned, NumericBaseType, NumericValue};

impl NumericAnyTypeOwned {
    pub fn human_name(&self) -> String {
        match self {
            NumericAnyTypeOwned::Base(base) => base.name(),
            NumericAnyTypeOwned::SubType { .. } => "SubType(todo)".to_string(),
            NumericAnyTypeOwned::ShiftScale { .. } => "ShiftScale(todo)".to_string(),
        }
    }

    pub fn default(&self) -> NumericValue {
        match self {
            NumericAnyTypeOwned::Base(base) => base.default(),
            NumericAnyTypeOwned::SubType { .. } => todo!(),
            NumericAnyTypeOwned::ShiftScale { .. } => todo!(),
        }
    }
}

impl NumericBaseType {
    pub fn name(&self) -> String {
        let name = match self {
            NumericBaseType::Nibble => "nib",
            NumericBaseType::U8 => "u8",
            NumericBaseType::U16 => "u16",
            NumericBaseType::U32 => "u32",
            NumericBaseType::UNib32 => "unib32",
            NumericBaseType::U64 => "u64",
            NumericBaseType::I32 => "i32",
            NumericBaseType::F32 => "f32",
            NumericBaseType::U128 => "u128",
            NumericBaseType::I8 => "i8",
            NumericBaseType::I16 => "i16",
            NumericBaseType::I64 => "i64",
            NumericBaseType::I128 => "i128",
            NumericBaseType::F16 => "f16",
            NumericBaseType::F64 => "f64",
            NumericBaseType::UB(bits) => {
                return format!("ub{}", bits.0);
            }
            NumericBaseType::IB(bits) => {
                return format!("ib{}", bits.0);
            }
            NumericBaseType::UN => "un",
            NumericBaseType::IN => "in",
            NumericBaseType::ULeb32 => "uleb32",
            NumericBaseType::ULeb64 => "uleb64",
            NumericBaseType::ULeb128 => "uleb128",
            NumericBaseType::ILeb32 => "ileb32",
            NumericBaseType::ILeb64 => "ileb64",
            NumericBaseType::ILeb128 => "ileb128",
            NumericBaseType::UQ { m, n } => {
                return format!("uq{m}.{n}");
            }
            NumericBaseType::IQ { m, n } => {
                return format!("iq{m}.{n}");
            }
        };
        name.to_string()
    }
}

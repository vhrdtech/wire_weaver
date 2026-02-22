#![cfg_attr(not(feature = "std"), no_std)]

use shrink_wrap::prelude::*;

#[derive_shrink_wrap]
#[ww_repr(unib32)]
#[self_describing]
#[derive(Clone, Debug, PartialEq, Eq)]
#[defmt = "defmt"]
#[serde = "serde"]
pub enum NumericBaseType {
    /// 4-bits (nibble), alignment of four-bits
    U4,
    /// u8 with alignment of one-byte
    U8,
    /// Little-Endian u16 with alignment of one-byte
    U16,
    /// Little-Endian u32 with alignment of one-byte
    U32,
    /// Variable length u32, alignment of four-bits, 1 to 11 nibbles
    UNib32,
    /// Little-Endian u64 with alignment of one-byte
    U64,
    /// Little-Endian i32 with alignment of one-byte
    I32,
    /// Little-Endian f32 with alignment of one-byte
    F32,

    /// Little-Endian u128 with alignment of one-byte
    U128,
    /// i8 with alignment of one-byte
    I8,
    /// Little-Endian i16 with alignment of one-byte
    I16,
    /// Little-Endian i64 with alignment of one-byte
    I64,
    /// Little-Endian i128 with alignment of one-byte
    I128,

    /// TODO: Little-Endian f16 with alignment of one-byte
    F16,
    /// Little-Endian f64 with alignment of one-byte
    F64,

    /// U1(bool)-U64, alignment of one-bit
    UB(UBits),
    /// I2-U64, alignment of one-bit
    IB(IBits),

    // TODO: or VLQ in Big Endian?
    ULeb32,
    ULeb64,
    ULeb128,
    ILeb32,
    ILeb64,
    ILeb128,

    /// TODO: Q-notation unsigned fixed point number, size = `m + n` bits, alignment of one-bit?
    UQ {
        // align_byte: bool, ?
        m: u8,
        n: u8,
    },
    /// TODO: Q-notation signed fixed point number, size = `1 + m + n` bits, alignment of one-bit?
    IQ {
        m: u8,
        n: u8,
    },
}

// It would be nice to create a separate SubType and ShiftScale for each base type,
// disallowing any ambiguities and errors on type level, but it would be too many variants to handle everywhere
/// Any of the base numeric types plus derived types: subtype, shift-scale.
#[derive_shrink_wrap]
#[ww_repr(unib32)]
#[derive(Clone, Debug)]
#[defmt = "defmt"]
#[owned = "std"]
#[serde = "serde"]
pub enum NumericAnyType<'i> {
    Base(NumericBaseType),
    SubType {
        base: NumericBaseType,
        kind: SubTypeKind<'i>,
    },
    // k*x + b?
    ShiftScale {
        base: NumericBaseType,
        shift: NumericValue,
        scale: NumericValue,
    },
}

#[derive_shrink_wrap]
#[ww_repr(unib32)]
#[derive(Clone, Debug)]
#[defmt = "defmt"]
#[owned = "std"]
#[serde = "serde"]
pub enum SubTypeKind<'i> {
    ValidRange {
        start: NumericValue,
        end: NumericValue,
    },
    ValidList(RefVec<'i, NumericValue>),
    InvalidList(RefVec<'i, NumericValue>),
}

/// Any number value (u8-u128, i8-i128, f32, f64, etc.).
/// Encoded as: (UNib32 type, number bytes).
///
/// Minimum size is 1 byte (u4, unib32 0..=7).
/// u8 is 2 bytes, u32 - is 5 bytes, etc.
#[derive_shrink_wrap]
#[ww_repr(unib32)]
#[self_describing]
#[derive(Copy, Clone, Debug, PartialEq)]
#[defmt = "defmt"]
#[serde = "serde"]
pub enum NumericValue {
    U4(Nibble),
    U8(u8),
    U16(u16),
    U32(u32),
    UNib32(u32),
    U64(u64),
    I32(i32),
    F32(f32),

    UN(UN),
    IN(IN),

    U128(u128),
    I8(i8),
    I16(i16),
    I64(i64),
    I128(i128),
    // F16,
    F64(f64),
    // TODO: the rest
}

impl NumericValue {
    pub fn ty(&self) -> NumericBaseType {
        use NumericBaseType::*;
        match self {
            NumericValue::U4(_) => U4,
            NumericValue::U8(_) => U8,
            NumericValue::U16(_) => U16,
            NumericValue::U32(_) => U32,
            NumericValue::UNib32(_) => UNib32,
            NumericValue::U64(_) => U64,
            NumericValue::I32(_) => I32,
            NumericValue::F32(_) => F32,
            NumericValue::UN(_) => todo!(),
            NumericValue::IN(_) => todo!(),
            NumericValue::U128(_) => U128,
            NumericValue::I8(_) => I8,
            NumericValue::I16(_) => I16,
            NumericValue::I64(_) => I64,
            NumericValue::I128(_) => I128,
            NumericValue::F64(_) => F64,
        }
    }

    pub fn as_f32(&self) -> f32 {
        match *self {
            NumericValue::U4(x) => x.value() as f32,
            NumericValue::U8(x) => x as f32,
            NumericValue::U16(x) => x as f32,
            NumericValue::U32(x) => x as f32,
            NumericValue::UNib32(x) => x as f32,
            NumericValue::U64(x) => x as f32,
            NumericValue::I32(x) => x as f32,
            NumericValue::F32(x) => x,
            NumericValue::UN(_x) => todo!(),
            NumericValue::IN(_x) => todo!(),
            NumericValue::U128(x) => x as f32,
            NumericValue::I8(x) => x as f32,
            NumericValue::I16(x) => x as f32,
            NumericValue::I64(x) => x as f32,
            NumericValue::I128(x) => x as f32,
            NumericValue::F64(x) => x as f32,
        }
    }
}

/// Number of bits in UB number. Serialized as 7-bits and shifted by -1 to represent U1-U128.
/// Note that only U1-U64 is supported now, but it's not hard to add numbers up to U128.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct UBits(u8);

impl SerializeShrinkWrap for UBits {
    const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: 7 };

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), ShrinkWrapError> {
        let shifted = self.0 - 1;
        wr.write_un8(7, shifted)
    }
}

impl<'i> DeserializeShrinkWrap<'i> for UBits {
    const ELEMENT_SIZE: ElementSize = <UBits as SerializeShrinkWrap>::ELEMENT_SIZE;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, ShrinkWrapError> {
        let shifted = rd.read_un8(7)?;
        if shifted <= 127 {
            Ok(UBits(shifted + 1))
        } else {
            Err(ShrinkWrapError::SubtypeOutOfRange)
        }
    }
}

impl DeserializeShrinkWrapOwned for UBits {
    const ELEMENT_SIZE: ElementSize = <UBits as SerializeShrinkWrap>::ELEMENT_SIZE;

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, ShrinkWrapError> {
        UBits::des_shrink_wrap(rd)
    }
}

/// Number of bits in IB number. Serialized as 7-bits and shifted by -2 to represent I2-I128.
/// Note that only I2-I64 is supported now, but it's not hard to add numbers up to I128.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct IBits(u8);

impl SerializeShrinkWrap for IBits {
    const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: 7 };

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), ShrinkWrapError> {
        let shifted = self.0 - 2;
        wr.write_un8(7, shifted)
    }
}

impl<'i> DeserializeShrinkWrap<'i> for IBits {
    const ELEMENT_SIZE: ElementSize = <IBits as SerializeShrinkWrap>::ELEMENT_SIZE;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, ShrinkWrapError> {
        let shifted = rd.read_un8(7)?;
        if shifted <= 126 {
            Ok(IBits(shifted + 2))
        } else {
            Err(ShrinkWrapError::SubtypeOutOfRange)
        }
    }
}

impl DeserializeShrinkWrapOwned for IBits {
    const ELEMENT_SIZE: ElementSize = <IBits as SerializeShrinkWrap>::ELEMENT_SIZE;

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, ShrinkWrapError> {
        IBits::des_shrink_wrap(rd)
    }
}

#[macro_export]
macro_rules! value {
    ($value:literal u8) => {
        ww_numeric::NumericValue::U8($value)
    };
    (relative_path $value:literal u8) => {
        NumericValue::U8($value)
    };
    ($value:literal u16) => {
        ww_numeric::NumericValue::U16($value)
    };
    (relative_path $value:literal u16) => {
        NumericValue::U16($value)
    };
    ($value:literal u32) => {
        ww_numeric::NumericValue::U32($value)
    };
    (relative_path $value:literal u32) => {
        NumericValue::U32($value)
    };
    ($value:literal u64) => {
        ww_numeric::NumericValue::U64($value)
    };
    (relative_path $value:literal u64) => {
        NumericValue::U64($value)
    };
    ($value:literal u128) => {
        ww_numeric::NumericValue::U128($value)
    };
    (relative_path $value:literal u128) => {
        NumericValue::U128($value)
    };
    ($value:literal i8) => {
        ww_numeric::NumericValue::I8($value)
    };
    (relative_path $value:literal i8) => {
        NumericValue::I8($value)
    };
    ($value:literal i16) => {
        ww_numeric::NumericValue::I16($value)
    };
    (relative_path $value:literal i16) => {
        NumericValue::I16($value)
    };
    ($value:literal i32) => {
        ww_numeric::NumericValue::I32($value)
    };
    (relative_path $value:literal i32) => {
        NumericValue::I32($value)
    };
    ($value:literal i64) => {
        ww_numeric::NumericValue::I64($value)
    };
    (relative_path $value:literal i64) => {
        NumericValue::I64($value)
    };
    ($value:literal i128) => {
        ww_numeric::NumericValue::I128($value)
    };
    (relative_path $value:literal i128) => {
        NumericValue::I128($value)
    };
    ($value:literal f32) => {
        ww_numeric::NumericValue::F32($value)
    };
    (relative_path $value:literal f32) => {
        NumericValue::F32($value)
    };
    ($value:literal f64) => {
        ww_numeric::NumericValue::F64($value)
    };
    (relative_path $value:literal f64) => {
        NumericValue::F64($value)
    };
}

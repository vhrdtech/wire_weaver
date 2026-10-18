#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "std")]
mod alloc;
mod store;

use shrink_wrap::prelude::*;
pub use shrink_wrap::{UVlq32, UVlq64};
pub use store::NumericValueStore;

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    cfg_attr_borrowed(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = unib32
)]
pub enum NumericBaseType {
    /// 4-bits (nibble), alignment of four-bits
    Nibble,
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

    /// U1(bool)-U64, alignment of one-bit. Number of bits are known and not serialized.
    UB(UBits),
    /// I2-I64, alignment of one-bit. Number of bits are known and not serialized.
    IB(IBits),

    /// u1-u64, aligned to one-bit. Number of bits are unknown and serialized.
    UN,
    /// i2-i64, aligned to one-bit. Number of bits are unknown and serialized.
    IN,

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

    /// [UVlq32](shrink_wrap::UVlq32): variable length u32, 1 to 5 bytes, alignment of one-byte
    UVlq32,
    /// [UVlq64](shrink_wrap::UVlq64): variable length u64, 1 to 10 bytes, alignment of one-byte
    UVlq64,
}

// It would be nice to create a separate SubType and ShiftScale for each base type,
// disallowing any ambiguities and errors on type level, but it would be too many variants to handle everywhere
/// Any of the base numeric types plus derived types: subtype, shift-scale.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_owned(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug, PartialEq),
    ww_repr = unib32,
    // a type description: readers can't skip a newer kind (SW-47, INT-8)
    no_future_version
)]
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

#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_owned(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug, PartialEq),
    ww_repr = unib32,
    // a type description: readers can't skip a newer kind (SW-47, INT-8)
    no_future_version
)]
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
///
/// Each variant but `U8` (so that there is always one) is behind the cargo feature of its name (`u16`, `f32`,
/// `unib32`, ..), `all` (part of `std`) enables them all. Firmware picks the ones it uses: the value is as big as the biggest variant enabled (`u128` makes it
/// 24 to 32 bytes) and unused number types bring no code. The bytes on the wire do not depend on the features;
/// a variant that is not enabled is read as [Error::EnumVariantDisabled](shrink_wrap::Error::EnumVariantDisabled).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    cfg_attr_borrowed(feature = "serde", derive(serde::Deserialize, serde::Serialize)),
    derive(Copy, Clone, Debug, PartialEq),
    non_exhaustive,
    self_describing,
    // a store by hand: one payload array as wide as the widest variant, not one per variant
    series(no_store),
    ww_repr = unib32
)]
pub enum NumericValue {
    #[cfg(feature = "nibble")]
    Nibble(Nibble),
    U8(u8),
    #[cfg(feature = "u16")]
    U16(u16),
    #[cfg(feature = "u32")]
    U32(u32),
    #[cfg(feature = "unib32")]
    UNib32(UNib32),
    #[cfg(feature = "u64")]
    U64(u64),
    #[cfg(feature = "i32")]
    I32(i32),
    #[cfg(feature = "f32")]
    F32(f32),

    #[cfg(feature = "un")]
    UN(UN),
    #[cfg(feature = "in")]
    IN(IN),

    #[cfg(feature = "u128")]
    U128(u128),
    #[cfg(feature = "i8")]
    I8(i8),
    #[cfg(feature = "i16")]
    I16(i16),
    #[cfg(feature = "i64")]
    I64(i64),
    #[cfg(feature = "i128")]
    I128(i128),
    // F16,
    #[cfg(feature = "f64")]
    F64(f64),

    /// Variable length u32 ([UVlq32]): 1 byte up to 127, 2 up to 16383, 5 at most. For values that are usually small
    /// but are not bounded.
    #[cfg(feature = "uvlq32")]
    UVlq32(UVlq32),
    /// Variable length u64 ([UVlq64]): 1 to 10 bytes.
    #[cfg(feature = "uvlq64")]
    UVlq64(UVlq64),
    // TODO: the rest
}

impl NumericValue {
    pub fn ty(&self) -> NumericBaseType {
        use NumericBaseType::*;
        match self {
            #[cfg(feature = "nibble")]
            NumericValue::Nibble(_) => Nibble,
            NumericValue::U8(_) => U8,
            #[cfg(feature = "u16")]
            NumericValue::U16(_) => U16,
            #[cfg(feature = "u32")]
            NumericValue::U32(_) => U32,
            #[cfg(feature = "unib32")]
            NumericValue::UNib32(_) => UNib32,
            #[cfg(feature = "u64")]
            NumericValue::U64(_) => U64,
            #[cfg(feature = "i32")]
            NumericValue::I32(_) => I32,
            #[cfg(feature = "f32")]
            NumericValue::F32(_) => F32,
            #[cfg(feature = "un")]
            NumericValue::UN(_) => todo!(),
            #[cfg(feature = "in")]
            NumericValue::IN(_) => todo!(),
            #[cfg(feature = "u128")]
            NumericValue::U128(_) => U128,
            #[cfg(feature = "i8")]
            NumericValue::I8(_) => I8,
            #[cfg(feature = "i16")]
            NumericValue::I16(_) => I16,
            #[cfg(feature = "i64")]
            NumericValue::I64(_) => I64,
            #[cfg(feature = "i128")]
            NumericValue::I128(_) => I128,
            #[cfg(feature = "f64")]
            NumericValue::F64(_) => F64,
            #[cfg(feature = "uvlq32")]
            NumericValue::UVlq32(_) => UVlq32,
            #[cfg(feature = "uvlq64")]
            NumericValue::UVlq64(_) => UVlq64,
        }
    }

    pub fn as_f32(&self) -> f32 {
        match *self {
            #[cfg(feature = "nibble")]
            NumericValue::Nibble(x) => x.value() as f32,
            NumericValue::U8(x) => x as f32,
            #[cfg(feature = "u16")]
            NumericValue::U16(x) => x as f32,
            #[cfg(feature = "u32")]
            NumericValue::U32(x) => x as f32,
            #[cfg(feature = "unib32")]
            NumericValue::UNib32(x) => x.0 as f32,
            #[cfg(feature = "u64")]
            NumericValue::U64(x) => x as f32,
            #[cfg(feature = "i32")]
            NumericValue::I32(x) => x as f32,
            #[cfg(feature = "f32")]
            NumericValue::F32(x) => x,
            #[cfg(feature = "un")]
            NumericValue::UN(_x) => todo!(),
            #[cfg(feature = "in")]
            NumericValue::IN(_x) => todo!(),
            #[cfg(feature = "u128")]
            NumericValue::U128(x) => x as f32,
            #[cfg(feature = "i8")]
            NumericValue::I8(x) => x as f32,
            #[cfg(feature = "i16")]
            NumericValue::I16(x) => x as f32,
            #[cfg(feature = "i64")]
            NumericValue::I64(x) => x as f32,
            #[cfg(feature = "i128")]
            NumericValue::I128(x) => x as f32,
            #[cfg(feature = "f64")]
            NumericValue::F64(x) => x as f32,
            #[cfg(feature = "uvlq32")]
            NumericValue::UVlq32(x) => x.0 as f32,
            #[cfg(feature = "uvlq64")]
            NumericValue::UVlq64(x) => x.0 as f32,
        }
    }

    /// The value as `f64`, rounded where it does not fit exactly (above 2^53, `u128`/`i128`).
    pub fn to_f64(&self) -> f64 {
        match *self {
            #[cfg(feature = "f32")]
            NumericValue::F32(x) => x as f64,
            #[cfg(feature = "f64")]
            NumericValue::F64(x) => x,
            #[cfg(feature = "u128")]
            NumericValue::U128(x) => x as f64,
            _ => match self.to_i128() {
                Some(x) => x as f64,
                None => f64::NAN,
            },
        }
    }

    /// The exact value of an integer variant, `None` for floats, the not yet supported `UN` / `IN`, and an `u128`
    /// above `i128::MAX`.
    pub fn to_i128(&self) -> Option<i128> {
        let x = match *self {
            #[cfg(feature = "nibble")]
            NumericValue::Nibble(x) => x.value() as i128,
            NumericValue::U8(x) => x as i128,
            #[cfg(feature = "u16")]
            NumericValue::U16(x) => x as i128,
            #[cfg(feature = "u32")]
            NumericValue::U32(x) => x as i128,
            #[cfg(feature = "unib32")]
            NumericValue::UNib32(x) => x.0 as i128,
            #[cfg(feature = "u64")]
            NumericValue::U64(x) => x as i128,
            #[cfg(feature = "i32")]
            NumericValue::I32(x) => x as i128,
            #[cfg(feature = "u128")]
            NumericValue::U128(x) => i128::try_from(x).ok()?,
            #[cfg(feature = "i8")]
            NumericValue::I8(x) => x as i128,
            #[cfg(feature = "i16")]
            NumericValue::I16(x) => x as i128,
            #[cfg(feature = "i64")]
            NumericValue::I64(x) => x as i128,
            #[cfg(feature = "i128")]
            NumericValue::I128(x) => x,
            #[cfg(feature = "uvlq32")]
            NumericValue::UVlq32(x) => x.0 as i128,
            #[cfg(feature = "uvlq64")]
            NumericValue::UVlq64(x) => x.0 as i128,
            #[allow(unreachable_patterns)]
            _ => return None,
        };
        Some(x)
    }

    /// Number of bits of an unsigned fixed-width variant (`Nibble` 4, `U8` 8, .., `U128` 128, `UNib32` and `UVlq32`
    /// 32, `UVlq64` 64): a counter stored in it wraps around at `2^bits`. `None` for signed and float variants.
    pub fn unsigned_bits(&self) -> Option<u32> {
        let bits = match self {
            #[cfg(feature = "nibble")]
            NumericValue::Nibble(_) => 4,
            NumericValue::U8(_) => 8,
            #[cfg(feature = "u16")]
            NumericValue::U16(_) => 16,
            #[cfg(feature = "u32")]
            NumericValue::U32(_) => 32,
            #[cfg(feature = "unib32")]
            NumericValue::UNib32(_) => 32,
            #[cfg(feature = "u64")]
            NumericValue::U64(_) => 64,
            #[cfg(feature = "u128")]
            NumericValue::U128(_) => 128,
            #[cfg(feature = "uvlq32")]
            NumericValue::UVlq32(_) => 32,
            #[cfg(feature = "uvlq64")]
            NumericValue::UVlq64(_) => 64,
            #[allow(unreachable_patterns)]
            _ => return None,
        };
        Some(bits)
    }

    /// True for `F32` and `F64`.
    pub fn is_float(&self) -> bool {
        match self {
            #[cfg(feature = "f32")]
            NumericValue::F32(_) => true,
            #[cfg(feature = "f64")]
            NumericValue::F64(_) => true,
            #[allow(unreachable_patterns)]
            _ => false,
        }
    }
}

macro_rules! from_number {
    ($($feature:literal $ty:ty => $variant:ident),*) => {
        $(
            #[cfg(feature = $feature)]
            impl From<$ty> for NumericValue {
                fn from(value: $ty) -> Self {
                    NumericValue::$variant(value)
                }
            }
        )*
    };
}

impl From<u8> for NumericValue {
    fn from(value: u8) -> Self {
        NumericValue::U8(value)
    }
}

from_number!(
    "nibble" Nibble => Nibble,
    "u16" u16 => U16,
    "u32" u32 => U32,
    "unib32" UNib32 => UNib32,
    "u64" u64 => U64,
    "i32" i32 => I32,
    "f32" f32 => F32,
    "u128" u128 => U128,
    "i8" i8 => I8,
    "i16" i16 => I16,
    "i64" i64 => I64,
    "i128" i128 => I128,
    "f64" f64 => F64,
    "uvlq32" UVlq32 => UVlq32,
    "uvlq64" UVlq64 => UVlq64
);

/// Number of bits in UB number. Serialized as 7-bits and shifted by -1 to represent U1-U128.
/// Note that only U1-U64 is supported now, but it's not hard to add numbers up to U128.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct UBits(pub u8);

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

#[cfg(feature = "std")]
impl SerializeShrinkWrapOwned for UBits {
    const ELEMENT_SIZE: ElementSize = <UBits as SerializeShrinkWrap>::ELEMENT_SIZE;

    fn ser_shrink_wrap_owned(&self, wr: &mut BufWriterOwned) -> Result<(), ShrinkWrapError> {
        let shifted = self.0 - 1;
        wr.write_un8(7, shifted)
    }
}

/// Number of bits in IB number. Serialized as 7-bits and shifted by -2 to represent I2-I128.
/// Note that only I2-I64 is supported now, but it's not hard to add numbers up to I128.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct IBits(pub u8);

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

#[cfg(feature = "std")]
impl SerializeShrinkWrapOwned for IBits {
    const ELEMENT_SIZE: ElementSize = <IBits as SerializeShrinkWrap>::ELEMENT_SIZE;

    fn ser_shrink_wrap_owned(&self, wr: &mut BufWriterOwned) -> Result<(), ShrinkWrapError> {
        let shifted = self.0 - 2;
        wr.write_un8(7, shifted)
    }
}

#[macro_export]
macro_rules! value {
    ($value:literal u8) => {
        ww_numeric::NumericValue::U8($value)
    };
    ($add_path:ident $value:literal u8) => {
        $add_path::ww_numeric::NumericValue::U8($value)
    };
    ($value:literal u16) => {
        ww_numeric::NumericValue::U16($value)
    };
    ($add_path:ident $value:literal u16) => {
        $add_path::ww_numeric::NumericValue::U16($value)
    };
    ($value:literal u32) => {
        ww_numeric::NumericValue::U32($value)
    };
    ($add_path:ident $value:literal u32) => {
        $add_path::ww_numeric::NumericValue::U32($value)
    };
    ($value:literal u64) => {
        ww_numeric::NumericValue::U64($value)
    };
    ($add_path:ident $value:literal u64) => {
        $add_path::ww_numeric::NumericValue::U64($value)
    };
    ($value:literal u128) => {
        ww_numeric::NumericValue::U128($value)
    };
    ($add_path:ident $value:literal u128) => {
        $add_path::ww_numeric::NumericValue::U128($value)
    };
    ($value:literal i8) => {
        ww_numeric::NumericValue::I8($value)
    };
    ($add_path:ident $value:literal i8) => {
        $add_path::ww_numeric::NumericValue::I8($value)
    };
    ($value:literal i16) => {
        ww_numeric::NumericValue::I16($value)
    };
    ($add_path:ident $value:literal i16) => {
        $add_path::ww_numeric::NumericValue::I16($value)
    };
    ($value:literal i32) => {
        ww_numeric::NumericValue::I32($value)
    };
    ($add_path:ident $value:literal i32) => {
        $add_path::ww_numeric::NumericValue::I32($value)
    };
    ($value:literal i64) => {
        ww_numeric::NumericValue::I64($value)
    };
    ($add_path:ident $value:literal i64) => {
        $add_path::ww_numeric::NumericValue::I64($value)
    };
    ($value:literal i128) => {
        ww_numeric::NumericValue::I128($value)
    };
    ($add_path:ident $value:literal i128) => {
        $add_path::ww_numeric::NumericValue::I128($value)
    };
    ($value:literal f32) => {
        ww_numeric::NumericValue::F32($value)
    };
    ($add_path:ident $value:literal f32) => {
        $add_path::ww_numeric::NumericValue::F32($value)
    };
    ($value:literal f64) => {
        ww_numeric::NumericValue::F64($value)
    };
    ($add_path:ident $value:literal f64) => {
        $add_path::ww_numeric::NumericValue::F64($value)
    };
    ($value:literal uvlq32) => {
        ww_numeric::NumericValue::UVlq32(ww_numeric::UVlq32($value))
    };
    ($add_path:ident $value:literal uvlq32) => {
        $add_path::ww_numeric::NumericValue::UVlq32($add_path::ww_numeric::UVlq32($value))
    };
    ($value:literal uvlq64) => {
        ww_numeric::NumericValue::UVlq64(ww_numeric::UVlq64($value))
    };
    ($add_path:ident $value:literal uvlq64) => {
        $add_path::ww_numeric::NumericValue::UVlq64($add_path::ww_numeric::UVlq64($value))
    };
}

impl NumericBaseType {
    pub fn default(&self) -> NumericValue {
        match self {
            #[cfg(feature = "nibble")]
            NumericBaseType::Nibble => NumericValue::Nibble(Nibble::zero()),
            NumericBaseType::U8 => NumericValue::U8(0),
            #[cfg(feature = "u16")]
            NumericBaseType::U16 => NumericValue::U16(0),
            #[cfg(feature = "u32")]
            NumericBaseType::U32 => NumericValue::U32(0),
            #[cfg(feature = "unib32")]
            NumericBaseType::UNib32 => NumericValue::UNib32(UNib32(0)),
            #[cfg(feature = "u64")]
            NumericBaseType::U64 => NumericValue::U64(0),
            #[cfg(feature = "i32")]
            NumericBaseType::I32 => NumericValue::I32(0),
            #[cfg(feature = "f32")]
            NumericBaseType::F32 => NumericValue::F32(0.0),
            #[cfg(feature = "u128")]
            NumericBaseType::U128 => NumericValue::U128(0),
            #[cfg(feature = "i8")]
            NumericBaseType::I8 => NumericValue::I8(0),
            #[cfg(feature = "i16")]
            NumericBaseType::I16 => NumericValue::I16(0),
            #[cfg(feature = "i64")]
            NumericBaseType::I64 => NumericValue::I64(0),
            #[cfg(feature = "i128")]
            NumericBaseType::I128 => NumericValue::I128(0),
            NumericBaseType::F16 => unimplemented!(),
            #[cfg(feature = "f64")]
            NumericBaseType::F64 => NumericValue::F64(0.0),
            NumericBaseType::UB(_) => unimplemented!(),
            NumericBaseType::IB(_) => unimplemented!(),
            #[cfg(feature = "un")]
            NumericBaseType::UN => NumericValue::UN(UN::UN8 {
                bit_count: U3::zero(),
                value: 0,
            }),
            #[cfg(feature = "in")]
            NumericBaseType::IN => NumericValue::IN(IN::IN8 {
                bit_count: U3::zero(),
                value: 0,
            }),
            #[cfg(feature = "uvlq32")]
            NumericBaseType::UVlq32 => NumericValue::UVlq32(UVlq32(0)),
            #[cfg(feature = "uvlq64")]
            NumericBaseType::UVlq64 => NumericValue::UVlq64(UVlq64(0)),
            // NumericBaseType::ULeb32 => {}
            // NumericBaseType::ULeb64 => {}
            // NumericBaseType::ULeb128 => {}
            // NumericBaseType::ILeb32 => {}
            // NumericBaseType::ILeb64 => {}
            // NumericBaseType::ILeb128 => {}
            // NumericBaseType::UQ { .. } => {}
            // NumericBaseType::IQ { .. } => {}
            _ => unimplemented!(),
        }
    }

    /// `x` in this integer type, wrapped around at its width like a counter (`U32` keeps `x mod 2^32`, `I8` the low
    /// 8 bits as two's complement, `UVlq32` and `UNib32` wrap at `2^32`): `None` for floats, a type that is not
    /// enabled, and `F16`, `UB`, `IB`, `UN`, `IN`.
    pub fn from_i128_wrapping(&self, x: i128) -> Option<NumericValue> {
        Some(match self {
            #[cfg(feature = "nibble")]
            NumericBaseType::Nibble => NumericValue::Nibble(Nibble::new((x & 0xf) as u8)?),
            NumericBaseType::U8 => NumericValue::U8(x as u8),
            #[cfg(feature = "u16")]
            NumericBaseType::U16 => NumericValue::U16(x as u16),
            #[cfg(feature = "u32")]
            NumericBaseType::U32 => NumericValue::U32(x as u32),
            #[cfg(feature = "unib32")]
            NumericBaseType::UNib32 => NumericValue::UNib32(UNib32(x as u32)),
            #[cfg(feature = "u64")]
            NumericBaseType::U64 => NumericValue::U64(x as u64),
            #[cfg(feature = "i32")]
            NumericBaseType::I32 => NumericValue::I32(x as i32),
            #[cfg(feature = "u128")]
            NumericBaseType::U128 => NumericValue::U128(x as u128),
            #[cfg(feature = "i8")]
            NumericBaseType::I8 => NumericValue::I8(x as i8),
            #[cfg(feature = "i16")]
            NumericBaseType::I16 => NumericValue::I16(x as i16),
            #[cfg(feature = "i64")]
            NumericBaseType::I64 => NumericValue::I64(x as i64),
            #[cfg(feature = "i128")]
            NumericBaseType::I128 => NumericValue::I128(x),
            #[cfg(feature = "uvlq32")]
            NumericBaseType::UVlq32 => NumericValue::UVlq32(UVlq32(x as u32)),
            #[cfg(feature = "uvlq64")]
            NumericBaseType::UVlq64 => NumericValue::UVlq64(UVlq64(x as u64)),
            _ => return None,
        })
    }

    /// `x` in this number type, exactly: `None` when it doesn't fit, for a type that is not enabled, and for
    /// `F16`, `UB`, `IB`, `UN`, `IN`. Floats take `x` rounded to their precision.
    pub fn from_i128(&self, x: i128) -> Option<NumericValue> {
        Some(match self {
            #[cfg(feature = "nibble")]
            NumericBaseType::Nibble => NumericValue::Nibble(Nibble::new(u8::try_from(x).ok()?)?),
            NumericBaseType::U8 => NumericValue::U8(x.try_into().ok()?),
            #[cfg(feature = "u16")]
            NumericBaseType::U16 => NumericValue::U16(x.try_into().ok()?),
            #[cfg(feature = "u32")]
            NumericBaseType::U32 => NumericValue::U32(x.try_into().ok()?),
            #[cfg(feature = "unib32")]
            NumericBaseType::UNib32 => NumericValue::UNib32(UNib32(x.try_into().ok()?)),
            #[cfg(feature = "u64")]
            NumericBaseType::U64 => NumericValue::U64(x.try_into().ok()?),
            #[cfg(feature = "i32")]
            NumericBaseType::I32 => NumericValue::I32(x.try_into().ok()?),
            #[cfg(feature = "f32")]
            NumericBaseType::F32 => NumericValue::F32(x as f32),
            #[cfg(feature = "u128")]
            NumericBaseType::U128 => NumericValue::U128(x.try_into().ok()?),
            #[cfg(feature = "i8")]
            NumericBaseType::I8 => NumericValue::I8(x.try_into().ok()?),
            #[cfg(feature = "i16")]
            NumericBaseType::I16 => NumericValue::I16(x.try_into().ok()?),
            #[cfg(feature = "i64")]
            NumericBaseType::I64 => NumericValue::I64(x.try_into().ok()?),
            #[cfg(feature = "i128")]
            NumericBaseType::I128 => NumericValue::I128(x),
            #[cfg(feature = "f64")]
            NumericBaseType::F64 => NumericValue::F64(x as f64),
            #[cfg(feature = "uvlq32")]
            NumericBaseType::UVlq32 => NumericValue::UVlq32(UVlq32(x.try_into().ok()?)),
            #[cfg(feature = "uvlq64")]
            NumericBaseType::UVlq64 => NumericValue::UVlq64(UVlq64(x.try_into().ok()?)),
            _ => return None,
        })
    }

    /// `x` in this number type: rounded to the nearest integer (halves away from zero) and saturated at the type's
    /// range for integers, `NaN` as zero; as is for floats. `None` for a type that is not enabled and for `F16`,
    /// `UB`, `IB`, `UN`, `IN`.
    pub fn from_f32_saturating(&self, x: f32) -> Option<NumericValue> {
        // `as` truncates toward zero and saturates, `NaN` becomes 0; `f32::round` is not in `core`
        #[allow(unused_variables)]
        let r = if x >= 0.0 { x + 0.5 } else { x - 0.5 };
        Some(match self {
            #[cfg(feature = "nibble")]
            NumericBaseType::Nibble => NumericValue::Nibble(Nibble::new_masked((r as u8).min(15))),
            NumericBaseType::U8 => NumericValue::U8(r as u8),
            #[cfg(feature = "u16")]
            NumericBaseType::U16 => NumericValue::U16(r as u16),
            #[cfg(feature = "u32")]
            NumericBaseType::U32 => NumericValue::U32(r as u32),
            #[cfg(feature = "unib32")]
            NumericBaseType::UNib32 => NumericValue::UNib32(UNib32(r as u32)),
            #[cfg(feature = "u64")]
            NumericBaseType::U64 => NumericValue::U64(r as u64),
            #[cfg(feature = "i32")]
            NumericBaseType::I32 => NumericValue::I32(r as i32),
            #[cfg(feature = "f32")]
            NumericBaseType::F32 => NumericValue::F32(x),
            #[cfg(feature = "u128")]
            NumericBaseType::U128 => NumericValue::U128(r as u128),
            #[cfg(feature = "i8")]
            NumericBaseType::I8 => NumericValue::I8(r as i8),
            #[cfg(feature = "i16")]
            NumericBaseType::I16 => NumericValue::I16(r as i16),
            #[cfg(feature = "i64")]
            NumericBaseType::I64 => NumericValue::I64(r as i64),
            #[cfg(feature = "i128")]
            NumericBaseType::I128 => NumericValue::I128(r as i128),
            #[cfg(feature = "f64")]
            NumericBaseType::F64 => NumericValue::F64(x as f64),
            #[cfg(feature = "uvlq32")]
            NumericBaseType::UVlq32 => NumericValue::UVlq32(UVlq32(r as u32)),
            #[cfg(feature = "uvlq64")]
            NumericBaseType::UVlq64 => NumericValue::UVlq64(UVlq64(r as u64)),
            _ => return None,
        })
    }
}

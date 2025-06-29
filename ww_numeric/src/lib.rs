use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::Error;

#[derive_shrink_wrap]
#[ww_repr(unib32)]
#[self_describing]
#[derive(Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
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
    /// Little-Endian u128 with alignment of one-byte
    U128,

    /// i8 with alignment of one-byte
    I8,
    /// Little-Endian i16 with alignment of one-byte
    I16,
    /// Little-Endian i32 with alignment of one-byte
    I32,
    /// Little-Endian i64 with alignment of one-byte
    I64,
    /// Little-Endian i128 with alignment of one-byte
    I128,

    /// TODO: Little-Endian f16 with alignment of one-byte
    F16,
    /// Little-Endian f32 with alignment of one-byte
    F32,
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
#[derive(Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
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
#[derive(Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum SubTypeKind<'i> {
    ValidRange {
        start: NumericValue,
        end: NumericValue,
    },
    ValidList(RefVec<'i, NumericValue>),
    InvalidList(RefVec<'i, NumericValue>),
}

#[derive_shrink_wrap]
#[ww_repr(unib32)]
#[self_describing]
#[derive(Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum NumericValue {
    U4(Nibble),
    U8(u8),
    U16(u16),
    I32(i32),
    U32(u32),
    UNib32(u32),
    UN(UN),
    IN(IN),

    I8(i8),
    U64(u64),
    U128(u128),
    I16(i16),
    I64(i64),
    I128(i128),
    // F16,
    F32(f32),
    F64(f64),
    // TODO: the rest
}

/// Number of bits in UB number. Serialized as 7-bits and shifted by -1 to represent U1-U128.
/// Note that only U1-U64 is supported now, but it's not hard to add numbers up to U128.
#[derive(Copy, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct UBits(u8);

impl SerializeShrinkWrap for UBits {
    const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: 7 };

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), ShrinkWrapError> {
        let shifted = self.0 - 1;
        wr.write_un8(7, shifted)
    }
}

impl<'i> DeserializeShrinkWrap<'i> for UBits {
    const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: 7 };

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, ShrinkWrapError> {
        let shifted = rd.read_un8(7)?;
        if shifted <= 127 {
            Ok(UBits(shifted + 1))
        } else {
            Err(Error::SubtypeOutOfRange)
        }
    }
}

/// Number of bits in IB number. Serialized as 7-bits and shifted by -2 to represent I2-I128.
/// Note that only I2-I64 is supported now, but it's not hard to add numbers up to I128.
#[derive(Copy, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct IBits(u8);

impl SerializeShrinkWrap for IBits {
    const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: 7 };

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), ShrinkWrapError> {
        let shifted = self.0 - 2;
        wr.write_un8(7, shifted)
    }
}

impl<'i> DeserializeShrinkWrap<'i> for IBits {
    const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: 7 };

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, ShrinkWrapError> {
        let shifted = rd.read_un8(7)?;
        if shifted <= 126 {
            Ok(IBits(shifted + 2))
        } else {
            Err(Error::SubtypeOutOfRange)
        }
    }
}

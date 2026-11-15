//! Sample codecs: how the values of a series column ([ColumnKind::Samples](crate::ColumnKind::Samples)) are coded.
//!
//! A column holds `count` samples of a [SampleShape]: `channels` elements each, interleaved, a complex element as its
//! two parts. Its [Codec] says how those scalar values (the column's *lanes* per sample) are laid out; the shape
//! comes from the track definition, so a codec id names only the byte transform. [Samples] reads a column in place
//! and decodes any range of samples; [encode] writes one.
//!
//! | id | codec | elements | layout |
//! |---:|---|---|---|
//! | 0 | [Codec::RAW] | all | little endian, `Bool` 8 per byte, first in bit 0 |
//! | 16 | [Codec::BIG_ENDIAN] | integers, floats | big endian |
//! | 17, 18 | [Codec::PACKED10_LSB], [Codec::PACKED10_MSB] | integers of 16 bits and more | 10 bits per value |
//! | 19, 20 | [Codec::PACKED12_LSB], [Codec::PACKED12_MSB] | integers of 16 bits and more | 12 bits per value |
//! | 21, 22 | [Codec::PACKED24_LSB], [Codec::PACKED24_MSB] | integers of 32 bits and more | 24 bits per value |
//! | 24 | [Codec::GORILLA] | floats | XOR with the previous value of the lane, Gorilla's control bits |
//! | 25 | [Codec::CHIMP] | floats | XOR with the previous value of the lane, Chimp's control bits |
//!
//! The raw device formats (0 to 22) are what a device sends, stored as it is and converted on read (a packed value
//! of a signed element is sign-extended); any sample reads in place. Value `i` of an `_LSB` column is bits
//! `i·w .. (i+1)·w` of the column read as one little-endian number (value 0 in the low bits of byte 0); of an `_MSB`
//! column bits `i·w ..` counted from the top bit of byte 0 (value 0 in the high bits of byte 0). Spare bits in the
//! last byte are zero.
//!
//! The XOR codecs (24, 25) are lossless for every bit pattern, NaN payloads included. Samples are coded in groups of
//! [XOR_GROUP] samples, each a bit stream padded to a byte that restarts the prediction, so a range decodes from its
//! group's start and a damaged byte costs only its group ([Samples::group_bytes]):
//!
//! ```text
//! groups × u32  end of each group's bytes, from the end of this table, ascending; the last is the stream's length
//! per group     lane by lane: the lane's first value as plain bits (32 or 64), then each later value of the lane
//!               as the codec's control bits and meaningful bits, against the lane's previous value; bits from the
//!               top of each byte down, zeros up to the next byte
//! ```
//!
//! Gorilla, per value, `x` the XOR with the previous value, `w` the value's bits (32 or 64): `0` when `x` is 0;
//! `10` and the meaningful bits of `x` in the previous window when they fit it; else `11`, the leading zeros (5 bits,
//! at most 31), the meaningful length (5 bits for `f32`, 6 for `f64`, `w` written as 0) and the meaningful bits. The
//! first value of a lane in a group opens no window, so its first nonzero XOR takes `11`.
//!
//! Chimp, per value: leading zeros rounded down to 0, 8, 12, 16, 18, 20, 22 or 24 (3 bits, the index); `00` when `x`
//! is 0; `01`, the index, the meaningful length (5 bits for `f32`, 6 for `f64`) and the meaningful bits when `x` has
//! more trailing zeros than 5 (`f32`) or 6 (`f64`); `10` and the `w` minus leading bits of `x` when the rounded leading
//! zeros equal the previous value's, kept from the last `11`; else `11`, the index and those bits. `00` and `01`
//! forget the kept leading zeros.
//!
//! Every codec is deterministic: the same values give the same bytes.

mod element;
mod raw;
#[cfg(test)]
mod tests;
mod xor;

pub use element::{Element, SampleShape};

use crate::Codec;
use core::ops::Range;

impl Codec {
    /// The element type big endian.
    pub const BIG_ENDIAN: Codec = Codec(16);
    /// 10 bits per value, the first value in the low bits.
    pub const PACKED10_LSB: Codec = Codec(17);
    /// 10 bits per value, the first value in the high bits.
    pub const PACKED10_MSB: Codec = Codec(18);
    /// 12 bits per value, the first value in the low bits (2 values in 3 bytes).
    pub const PACKED12_LSB: Codec = Codec(19);
    /// 12 bits per value, the first value in the high bits.
    pub const PACKED12_MSB: Codec = Codec(20);
    /// 24 bits per value, little endian.
    pub const PACKED24_LSB: Codec = Codec(21);
    /// 24 bits per value, big endian.
    pub const PACKED24_MSB: Codec = Codec(22);
    /// Floats, XOR with the previous value and Gorilla's control bits.
    pub const GORILLA: Codec = Codec(24);
    /// Floats, XOR with the previous value and Chimp's control bits.
    pub const CHIMP: Codec = Codec(25);
}

/// Samples per group of the XOR codecs.
pub const XOR_GROUP: usize = 1024;

/// A scalar type a column decodes into or encodes from.
pub trait Sample: Copy + Default + PartialEq + core::fmt::Debug + sealed::Sealed {
    /// The scalar [Element] of this type.
    const ELEMENT: Element;
    /// The value's bits, zero-extended.
    fn to_bits(self) -> u64;
    /// The value of the low bits of `bits`.
    fn from_bits(bits: u64) -> Self;
}

mod sealed {
    pub trait Sealed {}
}

macro_rules! sample_int {
    ($($t:ty => $e:ident, $u:ty);* $(;)?) => {$(
        impl sealed::Sealed for $t {}
        impl Sample for $t {
            const ELEMENT: Element = Element::$e;
            #[inline(always)]
            fn to_bits(self) -> u64 {
                self as $u as u64
            }
            #[inline(always)]
            fn from_bits(bits: u64) -> Self {
                bits as $u as $t
            }
        }
    )*};
}

sample_int! {
    i8 => I8, u8; i16 => I16, u16; i32 => I32, u32; i64 => I64, u64;
    u8 => U8, u8; u16 => U16, u16; u32 => U32, u32; u64 => U64, u64;
}

impl sealed::Sealed for f32 {}
impl Sample for f32 {
    const ELEMENT: Element = Element::F32;
    #[inline(always)]
    fn to_bits(self) -> u64 {
        u64::from(f32::to_bits(self))
    }
    #[inline(always)]
    fn from_bits(bits: u64) -> Self {
        f32::from_bits(bits as u32)
    }
}

impl sealed::Sealed for f64 {}
impl Sample for f64 {
    const ELEMENT: Element = Element::F64;
    #[inline(always)]
    fn to_bits(self) -> u64 {
        f64::to_bits(self)
    }
    #[inline(always)]
    fn from_bits(bits: u64) -> Self {
        f64::from_bits(bits)
    }
}

impl sealed::Sealed for bool {}
impl Sample for bool {
    const ELEMENT: Element = Element::Bool;
    #[inline(always)]
    fn to_bits(self) -> u64 {
        u64::from(self)
    }
    #[inline(always)]
    fn from_bits(bits: u64) -> Self {
        bits & 1 == 1
    }
}

/// Why a sample column can't be read or written.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SampleError {
    /// The codec doesn't code this element (or isn't a sample codec this reader knows).
    Unsupported { codec: Codec, element: Element },
    /// The shape has no channels, or more scalars than fit in memory.
    BadShape,
    /// The Rust type asked for is not the column's scalar.
    WrongType { column: Element, asked: Element },
    /// The column's length doesn't fit the codec and the count.
    BadLength { len: usize },
    /// The coded bits are malformed: a group runs past its end, or a field is out of range.
    Corrupt { group: usize },
    /// The samples asked for run past the column's count, or the output isn't whole samples.
    OutOfRange,
    /// A value doesn't fit the packed width (encoding), at this index of the values.
    ValueTooWide { index: usize },
    /// The output buffer is shorter than [max_encoded_len].
    Full { needed: usize },
}

/// How a codec codes values.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Kind {
    Le,
    Be,
    Packed { bits: u32, msb: bool },
    Xor(xor::Flavor),
}

fn kind(codec: Codec, element: Element) -> Result<Kind, SampleError> {
    let scalar = element.scalar();
    let bits = scalar.scalar_bits();
    let unsupported = Err(SampleError::Unsupported { codec, element });
    if bits == 0 {
        return unsupported;
    }
    let k = match codec {
        Codec::RAW if scalar == Element::Bool => Kind::Packed {
            bits: 1,
            msb: false,
        },
        Codec::RAW => Kind::Le,
        Codec::BIG_ENDIAN if scalar != Element::Bool => Kind::Be,
        Codec(id @ 17..=22) => {
            let w = [10, 10, 12, 12, 24, 24][usize::from(id - 17)];
            if scalar.is_float() || scalar == Element::Bool || bits < w {
                return unsupported;
            }
            Kind::Packed {
                bits: w,
                msb: id % 2 == 0,
            }
        }
        Codec::GORILLA if scalar.is_float() => Kind::Xor(xor::Flavor::Gorilla),
        Codec::CHIMP if scalar.is_float() => Kind::Xor(xor::Flavor::Chimp),
        _ => return unsupported,
    };
    Ok(k)
}

/// Scalars per sample of `shape`.
fn lanes(shape: SampleShape) -> Result<usize, SampleError> {
    shape
        .lanes()
        .and_then(|l| usize::try_from(l).ok())
        .ok_or(SampleError::BadShape)
}

/// Bits per value of a plain layout.
fn value_bits(kind: Kind, element: Element) -> u32 {
    match kind {
        Kind::Packed { bits, .. } => bits,
        _ => element.scalar_bits(),
    }
}

/// Bytes of `values` values of `bits` bits.
fn plain_len(values: usize, bits: u32) -> Option<usize> {
    values.checked_mul(bits as usize).map(|b| b.div_ceil(8))
}

/// The most bytes [encode] can write for `count` samples of `shape` in `codec`.
pub fn max_encoded_len(
    codec: Codec,
    shape: SampleShape,
    count: usize,
) -> Result<usize, SampleError> {
    let kind = kind(codec, shape.element)?;
    let values = count
        .checked_mul(lanes(shape)?)
        .ok_or(SampleError::BadShape)?;
    let len = match kind {
        Kind::Xor(_) => xor::max_len(count, values, shape.element.scalar_bits()),
        _ => plain_len(values, value_bits(kind, shape.element)),
    };
    len.ok_or(SampleError::BadShape)
}

/// Code `values` (whole samples of `shape`, lanes interleaved) in `codec` into `out`, which holds at least
/// [max_encoded_len] bytes; returns the bytes written.
pub fn encode<T: Sample>(
    codec: Codec,
    shape: SampleShape,
    values: &[T],
    out: &mut [u8],
) -> Result<usize, SampleError> {
    check_type::<T>(shape.element)?;
    let kind = kind(codec, shape.element)?;
    let lanes = lanes(shape)?;
    if !values.len().is_multiple_of(lanes) {
        return Err(SampleError::OutOfRange);
    }
    let needed = max_encoded_len(codec, shape, values.len() / lanes)?;
    if out.len() < needed {
        return Err(SampleError::Full { needed });
    }
    let out = &mut out[..needed];
    match kind {
        Kind::Le => Ok(raw::encode_le(values, out)),
        Kind::Be => Ok(raw::encode_be(values, out)),
        Kind::Packed { bits, msb } => raw::encode_packed(values, bits, msb, out),
        Kind::Xor(flavor) => Ok(xor::encode(flavor, values, lanes, out)),
    }
}

fn check_type<T: Sample>(element: Element) -> Result<(), SampleError> {
    if T::ELEMENT == element.scalar() {
        Ok(())
    } else {
        Err(SampleError::WrongType {
            column: element.scalar(),
            asked: T::ELEMENT,
        })
    }
}

/// A sample column read in place: its codec, shape and count, checked against its bytes.
#[derive(Copy, Clone, Debug)]
pub struct Samples<'b> {
    codec: Codec,
    shape: SampleShape,
    kind: Kind,
    lanes: usize,
    count: usize,
    bytes: &'b [u8],
}

impl<'b> Samples<'b> {
    /// Take `bytes` as `count` samples of `shape` coded in `codec`: the codec must code the element and the length
    /// must fit (for the XOR codecs, the group table too). No value is decoded yet.
    pub fn new(
        codec: Codec,
        shape: SampleShape,
        count: usize,
        bytes: &'b [u8],
    ) -> Result<Self, SampleError> {
        let kind = kind(codec, shape.element)?;
        let lanes = lanes(shape)?;
        let values = count.checked_mul(lanes).ok_or(SampleError::BadShape)?;
        let bad = SampleError::BadLength { len: bytes.len() };
        match kind {
            Kind::Xor(_) => xor::check_table(bytes, count)?,
            _ => {
                if plain_len(values, value_bits(kind, shape.element)) != Some(bytes.len()) {
                    return Err(bad);
                }
            }
        }
        Ok(Samples {
            codec,
            shape,
            kind,
            lanes,
            count,
            bytes,
        })
    }

    /// The column `column` of a chunk as `count` samples of `shape`.
    pub fn of(
        column: &crate::chunk::Column<'b>,
        shape: SampleShape,
        count: usize,
    ) -> Result<Self, SampleError> {
        Self::new(column.codec, shape, count, column.bytes)
    }

    pub fn codec(&self) -> Codec {
        self.codec
    }

    pub fn shape(&self) -> SampleShape {
        self.shape
    }

    /// Samples in the column.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Scalars per sample.
    pub fn lanes(&self) -> usize {
        self.lanes
    }

    /// Decode samples from `first` on into `out`, whole samples of interleaved lanes: `out.len() / lanes` samples.
    pub fn decode<T: Sample>(&self, first: usize, out: &mut [T]) -> Result<(), SampleError> {
        check_type::<T>(self.shape.element)?;
        if !out.len().is_multiple_of(self.lanes) {
            return Err(SampleError::OutOfRange);
        }
        let n = out.len() / self.lanes;
        match first.checked_add(n) {
            Some(end) if end <= self.count => {}
            _ => return Err(SampleError::OutOfRange),
        }
        let first_value = first * self.lanes;
        match self.kind {
            Kind::Le => raw::decode_le(self.bytes, first_value, out),
            Kind::Be => raw::decode_be(self.bytes, first_value, out),
            Kind::Packed { bits, msb } => raw::decode_packed(
                self.bytes,
                bits,
                msb,
                self.shape.element.is_signed(),
                first_value,
                out,
            ),
            Kind::Xor(flavor) => {
                return xor::decode(flavor, self.bytes, self.count, self.lanes, first, out);
            }
        }
        Ok(())
    }

    /// Groups a reader can decode alone: 1 for a raw format, a group per [XOR_GROUP] samples for the XOR codecs.
    pub fn group_count(&self) -> usize {
        match self.kind {
            Kind::Xor(_) => self.count.div_ceil(XOR_GROUP),
            _ => 1,
        }
    }

    /// The samples of group `g` and the bytes of the column that code them: a reader whose chunk has bad segments
    /// ([Chunk::range_damaged](crate::Chunk::range_damaged), offset by the column's offset) decodes the groups whose
    /// bytes are intact. A raw format's samples each decode alone: [Self::sample_bytes] narrows its single group.
    ///
    /// # Panics
    ///
    /// When `g` is not below [Self::group_count].
    pub fn group_bytes(&self, g: usize) -> (Range<usize>, Range<usize>) {
        assert!(
            g < self.group_count(),
            "group {g} of {}",
            self.group_count()
        );
        match self.kind {
            Kind::Xor(_) => {
                let samples = g * XOR_GROUP..self.count.min((g + 1) * XOR_GROUP);
                (samples, xor::group_range(self.bytes, self.count, g))
            }
            _ => (0..self.count, 0..self.bytes.len()),
        }
    }

    /// The bytes that code `samples` in a raw format, None for the XOR codecs (see [Self::group_bytes]).
    pub fn sample_bytes(&self, samples: Range<usize>) -> Option<Range<usize>> {
        let bits = match self.kind {
            Kind::Xor(_) => return None,
            k => value_bits(k, self.shape.element) as usize,
        };
        let from = samples.start * self.lanes * bits / 8;
        let to = (samples.end * self.lanes * bits).div_ceil(8);
        Some(from..to.min(self.bytes.len()))
    }
}

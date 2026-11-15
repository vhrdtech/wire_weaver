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
//! | 24, 25 | | | reserved, refused as an unknown codec |
//! | 26 | [Codec::ALP] | floats | decimals as integers bit-packed per group, else front bits via a dictionary ([alp]) |
//! | 32 | [Codec::DELTA] | integers | differences, frame of reference, bit-packed per group |
//! | 33 | [Codec::DELTA2] | integers | differences of differences, as [Codec::DELTA] |
//! | 34, 35 | | | reserved, refused as an unknown codec |
//! | 36 | [Codec::RUNS] | `U8`, `U16`, `U32`, `Bool` | reserved: run-coded discrete lanes, refused until built |
//!
//! The raw device formats (0 to 22) are what a device sends, stored as it is and converted on read (a packed value
//! of a signed element is sign-extended); any sample reads in place. Value `i` of an `_LSB` column is bits
//! `i·w .. (i+1)·w` of the column read as one little-endian number (value 0 in the low bits of byte 0); of an `_MSB`
//! column bits `i·w ..` counted from the top bit of byte 0 (value 0 in the high bits of byte 0). Spare bits in the
//! last byte are zero.
//!
//! The delta codecs (32, 33) code groups of [DELTA_GROUP] samples on the shared [bitpack](crate::bitpack) primitive.
//! Each group restarts, so a range decodes from its group's start and a damaged byte costs only its group
//! ([Samples::group_bytes]):
//!
//! ```text
//! groups × u32  end of each group's bytes, from the end of this table, ascending; the last is the stream's length
//! per group, lane by lane (b = bytes of the scalar, n = samples in the group):
//!   b      the lane's first value
//!   b      order 2: its first difference (0 when n is 1)
//!   b      min: the smallest difference (order 1) or difference of differences (order 2), as a signed number
//!   u8     width, at most the scalar's bits
//!   ...    the n - order differences minus min, bit-packed at width (horizontal, LSB first), padded to a byte
//! ```
//!
//! Differences wrap in the scalar's bits, so every value codes, and the minimum is taken as a signed number, so a
//! ramp or a constant packs at width 0.
//!
//! ALP (26), the one float codec, codes groups of [ALP_GROUP] samples with the same table of group ends; its layout
//! is in [alp]. Ids 24, 25, 34 and 35 are reserved; a reader refuses them as unknown codecs.
//!
//! Every codec is deterministic: the same values give the same bytes.

pub mod alp;
mod delta;
mod element;
mod raw;
#[cfg(test)]
mod tests;

pub use alp::ALP_GROUP;
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
    /// Floats, ALP: decimals as integers bit-packed per group, other floats' front bits through a dictionary.
    pub const ALP: Codec = Codec(26);
    /// Integers, differences bit-packed per group.
    pub const DELTA: Codec = Codec(32);
    /// Integers, differences of differences bit-packed per group.
    pub const DELTA2: Codec = Codec(33);
    /// Reserved: run-coded discrete lanes (`U8`, `U16`, `U32`, `Bool`); a reader refuses it until it is built.
    pub const RUNS: Codec = Codec(36);
}

/// Samples per group of the delta codecs: a [bitpack](crate::bitpack) group.
pub const DELTA_GROUP: usize = crate::bitpack::GROUP;

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
    Delta(delta::Flavor),
    Alp,
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
        Codec::ALP if scalar.is_float() => Kind::Alp,
        Codec(id @ 32..=33) => {
            if scalar.is_float() || scalar == Element::Bool {
                return unsupported;
            }
            Kind::Delta(delta::Flavor { order: id - 31 })
        }
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
        Kind::Delta(f) => delta::max_len(f.order, count, values, shape.element.scalar_bits()),
        Kind::Alp => alp::max_len(count, values, shape.element.scalar_bits()),
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
        Kind::Delta(flavor) => Ok(delta::encode(flavor, values, lanes, out)),
        Kind::Alp => alp::encode(values, lanes, out),
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

/// Bytes of a group table entry.
const ENTRY: usize = 4;

/// Check the table of a grouped column of `groups` groups: ascending ends, the last one the length after the table.
fn check_table(bytes: &[u8], groups: usize) -> Result<(), SampleError> {
    let bad = SampleError::BadLength { len: bytes.len() };
    let table = groups.checked_mul(ENTRY).ok_or(bad)?;
    if bytes.len() < table {
        return Err(bad);
    }
    let mut prev = 0usize;
    for e in bytes[..table].as_chunks::<ENTRY>().0 {
        let end = u32::from_le_bytes(*e) as usize;
        if end < prev {
            return Err(bad);
        }
        prev = end;
    }
    if prev != bytes.len() - table {
        return Err(bad);
    }
    Ok(())
}

/// The bytes of group `g` of a grouped column, None when the table doesn't hold it.
fn table_entry(bytes: &[u8], groups: usize, g: usize) -> Option<Range<usize>> {
    let table = groups.checked_mul(ENTRY)?;
    let end_of = |i: usize| -> Option<usize> {
        let at = i.checked_mul(ENTRY)?;
        let e = bytes.get(at..at.checked_add(ENTRY)?)?;
        let e: [u8; ENTRY] = e.try_into().ok()?;
        table.checked_add(u32::from_le_bytes(e) as usize)
    };
    let from = match g.checked_sub(1) {
        Some(p) => end_of(p)?,
        None => table,
    };
    let to = end_of(g)?;
    (g < groups && from <= to && to <= bytes.len()).then_some(from..to)
}

/// The bytes of group `g` of a grouped column whose table was checked.
fn table_range(bytes: &[u8], groups: usize, g: usize) -> Range<usize> {
    table_entry(bytes, groups, g).expect("a checked table holds every group")
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
    /// must fit (for the grouped codecs, the group table too). No value is decoded yet.
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
            Kind::Delta(_) => delta::check_table(bytes, count)?,
            Kind::Alp => alp::check_table(bytes, count)?,
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
            Kind::Delta(flavor) => {
                return delta::decode(flavor, self.bytes, self.count, self.lanes, first, out);
            }
            Kind::Alp => return alp::decode(self.bytes, self.count, self.lanes, first, out),
        }
        Ok(())
    }

    /// Samples per group: [DELTA_GROUP] or [ALP_GROUP], None for a raw format.
    fn group_len(&self) -> Option<usize> {
        match self.kind {
            Kind::Delta(_) => Some(DELTA_GROUP),
            Kind::Alp => Some(ALP_GROUP),
            _ => None,
        }
    }

    /// Groups a reader can decode alone: 1 for a raw format, a group per [DELTA_GROUP] or [ALP_GROUP]
    /// samples for the delta and ALP codecs.
    pub fn group_count(&self) -> usize {
        self.group_len().map_or(1, |n| self.count.div_ceil(n))
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
        match self.group_len() {
            Some(n) => {
                let samples = g * n..self.count.min((g + 1) * n);
                (samples, table_range(self.bytes, self.count.div_ceil(n), g))
            }
            None => (0..self.count, 0..self.bytes.len()),
        }
    }

    /// The bytes that code `samples` in a raw format, None for the grouped codecs (see [Self::group_bytes]).
    pub fn sample_bytes(&self, samples: Range<usize>) -> Option<Range<usize>> {
        let bits = match self.kind {
            Kind::Delta(_) | Kind::Alp => return None,
            k => value_bits(k, self.shape.element) as usize,
        };
        let from = samples.start * self.lanes * bits / 8;
        let to = (samples.end * self.lanes * bits).div_ceil(8);
        Some(from..to.min(self.bytes.len()))
    }
}

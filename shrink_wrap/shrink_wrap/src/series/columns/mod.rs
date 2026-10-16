//! Column codecs of `Series<T>` (SW-60): the building blocks a `Series` writes and reads its columns with, one
//! column per leaf field of `T`. The wire layout is fixed here; `Series<'i, T>` and the `SeriesColumns` trait put
//! the columns together.
//!
//! - [numeric]: integers up to 64 bits, a 4-bit header (predictor raw / delta / delta of delta, coder prefix /
//!   packed / constant / plain) and the values split into segments at restarts; 128-bit integers raw + plain only.
//! - [float]: `f32` / `f64`, plain or the SW-32 XOR code, segments at restarts.
//! - [run]: discrete values (enum tags, bools, opaque bytes) as a first value and one entry per change.
//!
//! Every code goes through the bit cursor, most significant bit first, with no alignment (opaque run values align
//! to a byte). Writers take an iterator over the column's items (with the restart flag of each where the column has
//! segments) and walk it more than once (codec choice, packed block widths), so it must be `Clone`; they allocate
//! nothing. Readers own a copy of the [BufReader] positioned at the column and are stepped one item at a time by
//! the caller, which knows the item count and the restarts (from the runs before the column). Malformed data is an
//! [Error] (`MalformedSeries` or an out-of-bounds read), never a panic, and every read is bounded by
//! the number of items stepped.

pub(crate) mod float;
pub(crate) mod numeric;
pub(crate) mod run;

#[cfg(any(test, fuzzing))]
pub mod check;
#[cfg(test)]
mod golden;
#[cfg(test)]
mod tests;

use super::BitSink;
use crate::{BufReader, Error};

/// What the 2-bit predictor field of a numeric column names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Predictor {
    /// Every value is a residual: its zigzag for signed types, the value itself for unsigned ones.
    Raw = 0,
    /// The first value of a segment at full width, then the zigzag of the difference to the previous value.
    Delta = 1,
    /// The first value of a segment at full width, then the zigzag of the change of the difference.
    DeltaOfDelta = 2,
}

/// What the 2-bit coder field of a numeric column names: how residuals are written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Coder {
    /// Each residual in the SW-31 delta code.
    Prefix = 0,
    /// Blocks of [PACKED_BLOCK] residuals of a segment, each block a width field and the residuals at that width.
    Packed = 1,
    /// The first residual in the SW-31 delta code, every later one equal to it and not written.
    Constant = 2,
    /// Each residual at the full width.
    Plain = 3,
}

/// Predictor and coder of a numeric column: its 4-bit header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct NumericCodec {
    pub predictor: Predictor,
    pub coder: Coder,
}

impl NumericCodec {
    pub const fn new(predictor: Predictor, coder: Coder) -> Self {
        NumericCodec { predictor, coder }
    }
}

/// The 4-bit codec field of a float column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum FloatCodec {
    /// Each value at full width.
    Plain = 0,
    /// The SW-32 `XorFloat` code, the first value of each segment at full width.
    Xor = 1,
}

/// How a writer picks the codec of each column.
///
/// `None` tries every candidate (counting bits only, no scratch memory) and writes the smallest: CPU about
/// columns x candidates x items. A fixed codec skips the trials, for a small MCU: it is used for every column,
/// except where it can't encode one (constant over residuals that vary: the same predictor with packed instead;
/// 128-bit columns: always raw + plain).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct CodecPolicy {
    pub numeric: Option<NumericCodec>,
    pub float: Option<FloatCodec>,
}

impl CodecPolicy {
    /// The smallest encoding of every column.
    pub const SMALLEST: CodecPolicy = CodecPolicy {
        numeric: None,
        float: None,
    };

    /// The same codecs for every column, no trials.
    pub const fn fixed(numeric: NumericCodec, float: FloatCodec) -> Self {
        CodecPolicy {
            numeric: Some(numeric),
            float: Some(float),
        }
    }
}

impl Default for CodecPolicy {
    fn default() -> Self {
        CodecPolicy::SMALLEST
    }
}

/// Residuals per block of the packed coder.
pub const PACKED_BLOCK: u8 = 8;

/// A [BitSink] that only counts, to compare codecs or to measure a group without writing.
#[derive(Clone, Copy, Debug)]
pub struct BitCounter {
    start: usize,
    pos: usize,
}

impl BitCounter {
    /// Counts from a byte boundary (only [align_byte](BitSink::align_byte) depends on the start).
    pub const fn new() -> Self {
        BitCounter::at(0)
    }

    /// Counts from bit position `start` of a buffer, so that byte alignment comes out as it would there.
    pub const fn at(start: usize) -> Self {
        BitCounter { start, pos: start }
    }

    /// Bits counted since the start.
    pub const fn bits(&self) -> usize {
        self.pos - self.start
    }
}

impl Default for BitCounter {
    fn default() -> Self {
        BitCounter::new()
    }
}

impl BitSink for BitCounter {
    fn bit_pos(&self) -> usize {
        self.pos
    }

    fn put_bit(&mut self, _bit: bool) -> Result<(), Error> {
        self.pos += 1;
        Ok(())
    }

    fn put_bits(&mut self, count: u8, _value: u64) -> Result<(), Error> {
        self.pos += count as usize;
        Ok(())
    }

    fn align_byte(&mut self) {
        self.pos = self.pos.next_multiple_of(8);
    }

    fn put_bytes(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.align_byte();
        self.pos += bytes.len() * 8;
        Ok(())
    }
}

/// `nib(x)`: the UNib32 code through the bit cursor without nibble alignment, groups of 4 bits, least significant
/// group first, `0b1000` in a group when one more follows: 0..=7 in 4 bits, up to 63 in 8, up to 511 in 12.
pub(crate) fn put_nib(sink: &mut impl BitSink, mut value: u32) -> Result<(), Error> {
    loop {
        let more = value >> 3 != 0;
        sink.put_bits(4, (value & 0b111) as u64 | if more { 0b1000 } else { 0 })?;
        value >>= 3;
        if !more {
            return Ok(());
        }
    }
}

/// Reads a [put_nib] code: at most 11 groups, and the last one may not carry bits above `u32`.
pub(crate) fn get_nib(rd: &mut BufReader<'_>) -> Result<u32, Error> {
    let mut value = 0u32;
    for group in 0..10 {
        let nib = rd.read_un32(4)?;
        value |= (nib & 0b111) << (3 * group);
        if nib & 0b1000 == 0 {
            return Ok(value);
        }
    }
    // the 11th group holds bits 30 and 31 and is the last one
    let nib = rd.read_un32(4)?;
    if nib > 0b11 {
        return Err(Error::MalformedSeries);
    }
    Ok(value | nib << 30)
}

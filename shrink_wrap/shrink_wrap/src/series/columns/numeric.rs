//! Numeric column: integers of 1 to 64 bits, and 128-bit ones raw + plain.
//!
//! ```text
//! predictor:  2 bits   00 raw, 01 delta, 10 delta of delta, 11 escape (reserved in v1: malformed)
//! coder:      2 bits   00 prefix, 01 packed, 10 constant, 11 plain
//! values:     one per item
//! ```
//!
//! Values split into segments at restarts. With delta or delta of delta, the first value of a segment is written
//! at the full width W and every other one as a residual: the zigzag of the difference to the previous value, or
//! of the change of that difference (the previous difference is 0 at a segment start), wrapped at W as SW-31 does.
//! With raw there are no segments (restarts change nothing) and every value is a residual: its zigzag for signed
//! types, the value itself for unsigned ones. Coders, for the residuals: prefix is the SW-31 delta code at W;
//! packed writes the residuals of a segment in blocks of 8 (the last one of a segment shorter), each a width
//! (0..=W) in `bits(W)` bits and the residuals at that width; constant writes the first residual of the column in
//! the prefix code and no other (all equal it); plain writes each at W.

use super::{BitCounter, Coder, NumericCodec, PACKED_BLOCK, Predictor};
use crate::series::{BitSink, get_delta, mask, put_delta, sign_extend, unzigzag, zigzag};
use crate::{BufReader, Error};

/// Width (1 to 64 bits) and signedness of a numeric column's values, from the leaf type (`u8`: 8 unsigned,
/// `I12`: 12 signed, `UNib32`: 32 unsigned).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub(crate) struct NumericKind {
    width: u8,
    signed: bool,
}

impl NumericKind {
    /// Panics on a width outside 1..=64: a bug in the caller, never data.
    pub(crate) const fn new(width: u8, signed: bool) -> Self {
        assert!(
            width >= 1 && width <= 64,
            "numeric column width is 1 to 64 bits"
        );
        NumericKind { width, signed }
    }
}

/// The predictors and coders in the order the writer tries them: on a tie, the first one wins.
const PREDICTORS: [Predictor; 3] = [Predictor::Raw, Predictor::Delta, Predictor::DeltaOfDelta];
const CODERS: [Coder; 4] = [Coder::Prefix, Coder::Packed, Coder::Constant, Coder::Plain];

/// Bits of the packed block width field, enough for 0..=W: 4 for W = 8, 5 for 16, 6 for 32, 7 for 64.
const fn block_width_bits(width: u8) -> u8 {
    8 - width.leading_zeros() as u8
}

/// What the predictor makes of one item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// A segment start, the value's bits at full width.
    Start(u64),
    /// A residual, below `2^W`.
    Residual(u64),
}

/// The predictor over a column's items: segment starts and residuals.
#[derive(Clone)]
struct Steps<I> {
    items: I,
    kind: NumericKind,
    predictor: Predictor,
    prev: Option<u64>,
    prev_delta: i64,
}

impl<I: Iterator<Item = (u64, bool)>> Steps<I> {
    fn new(items: I, kind: NumericKind, predictor: Predictor) -> Self {
        Steps {
            items,
            kind,
            predictor,
            prev: None,
            prev_delta: 0,
        }
    }
}

impl<I: Iterator<Item = (u64, bool)>> Iterator for Steps<I> {
    type Item = Step;

    fn next(&mut self) -> Option<Step> {
        let (value, restart) = self.items.next()?;
        let w = self.kind.width;
        let bits = value & mask(w);
        if self.predictor == Predictor::Raw {
            return Some(Step::Residual(if self.kind.signed {
                zigzag(sign_extend(bits, w)) & mask(w)
            } else {
                bits
            }));
        }
        let step = match self.prev {
            Some(prev) if !restart => {
                let delta = sign_extend(bits.wrapping_sub(prev) & mask(w), w);
                let coded = if self.predictor == Predictor::DeltaOfDelta {
                    sign_extend((delta.wrapping_sub(self.prev_delta) as u64) & mask(w), w)
                } else {
                    delta
                };
                self.prev_delta = delta;
                Step::Residual(zigzag(coded) & mask(w))
            }
            _ => {
                self.prev_delta = 0;
                Step::Start(bits)
            }
        };
        self.prev = Some(bits);
        Some(step)
    }
}

/// Whether every residual of the column under `predictor` is the same (or there is none), so that the constant
/// coder can write it.
fn constant_fits<I>(kind: NumericKind, predictor: Predictor, items: I) -> bool
where
    I: Iterator<Item = (u64, bool)>,
{
    let mut first = None;
    for step in Steps::new(items, kind, predictor) {
        if let Step::Residual(z) = step {
            match first {
                None => first = Some(z),
                Some(f) if f != z => return false,
                Some(_) => {}
            }
        }
    }
    true
}

/// Writes the column with `codec`, which the caller checked fits (constant).
fn encode<I>(
    sink: &mut impl BitSink,
    kind: NumericKind,
    codec: NumericCodec,
    items: I,
) -> Result<(), Error>
where
    I: Iterator<Item = (u64, bool)>,
{
    let w = kind.width;
    sink.put_bits(4, ((codec.predictor as u64) << 2) | codec.coder as u64)?;
    let mut steps = Steps::new(items, kind, codec.predictor);
    if codec.coder == Coder::Packed {
        return encode_packed(sink, w, steps);
    }
    let mut constant_written = false;
    for step in steps.by_ref() {
        let z = match step {
            Step::Start(bits) => {
                sink.put_bits(w, bits)?;
                continue;
            }
            Step::Residual(z) => z,
        };
        match codec.coder {
            Coder::Prefix => put_delta(sink, z, w)?,
            Coder::Plain => sink.put_bits(w, z)?,
            Coder::Constant => {
                if !constant_written {
                    put_delta(sink, z, w)?;
                    constant_written = true;
                }
            }
            Coder::Packed => unreachable!("written by encode_packed"),
        }
    }
    Ok(())
}

/// The packed coder's values: segment starts at full width, residuals in blocks of up to [PACKED_BLOCK] (a block
/// ends early at a segment start or the column end), each block its width and the residuals at that width. The
/// residuals of a block are kept until its width is known, so the predictor runs once per item.
fn encode_packed<I>(sink: &mut impl BitSink, w: u8, mut steps: Steps<I>) -> Result<(), Error>
where
    I: Iterator<Item = (u64, bool)>,
{
    let mut block = [0u64; PACKED_BLOCK as usize];
    let mut next = steps.next();
    while let Some(step) = next {
        match step {
            Step::Start(bits) => {
                sink.put_bits(w, bits)?;
                next = steps.next();
            }
            Step::Residual(z) => {
                block[0] = z;
                let mut len = 1;
                let mut any_bits = z;
                next = steps.next();
                while len < PACKED_BLOCK as usize {
                    let Some(Step::Residual(z)) = next else {
                        break;
                    };
                    block[len] = z;
                    any_bits |= z;
                    len += 1;
                    next = steps.next();
                }
                let block_width = (64 - any_bits.leading_zeros()) as u8;
                sink.put_bits(block_width_bits(w), block_width as u64)?;
                for &z in &block[..len] {
                    sink.put_bits(block_width, z)?;
                }
            }
        }
    }
    Ok(())
}

/// Bits the column takes with `codec` (header included), `None` if the codec can't encode it.
pub(crate) fn numeric_bits<I>(kind: NumericKind, codec: NumericCodec, items: I) -> Option<usize>
where
    I: Iterator<Item = (u64, bool)> + Clone,
{
    if codec.coder == Coder::Constant && !constant_fits(kind, codec.predictor, items.clone()) {
        return None;
    }
    let mut counter = BitCounter::new();
    encode(&mut counter, kind, codec, items).ok()?;
    Some(counter.bits())
}

/// The codec with the fewest bits for the column, trying predictors raw, delta, delta of delta, each with coders
/// prefix, packed, constant, plain; the first of equal ones wins.
pub(crate) fn smallest_numeric<I>(kind: NumericKind, items: I) -> NumericCodec
where
    I: Iterator<Item = (u64, bool)> + Clone,
{
    let mut best = (NumericCodec::new(Predictor::Raw, Coder::Plain), usize::MAX);
    for predictor in PREDICTORS {
        for coder in CODERS {
            let codec = NumericCodec::new(predictor, coder);
            if let Some(bits) = numeric_bits(kind, codec, items.clone())
                && bits < best.1
            {
                best = (codec, bits);
            }
        }
    }
    best.0
}

/// Writes a numeric column of 1- to 64-bit values: `items` are each item's value bits (bits above the width are
/// ignored, signed values sign-extended or not) and whether it restarts the column. `codec` `None` picks the
/// smallest; a fixed constant that doesn't fit falls back to packed with the same predictor. Returns the codec
/// written. Writes nothing but the header for no items: a `Series` doesn't write empty columns at all.
pub(crate) fn write_numeric<I>(
    sink: &mut impl BitSink,
    kind: NumericKind,
    items: I,
    codec: Option<NumericCodec>,
) -> Result<NumericCodec, Error>
where
    I: Iterator<Item = (u64, bool)> + Clone,
{
    let codec = match codec {
        None => smallest_numeric(kind, items.clone()),
        Some(c)
            if c.coder == Coder::Constant && !constant_fits(kind, c.predictor, items.clone()) =>
        {
            NumericCodec::new(c.predictor, Coder::Packed)
        }
        Some(c) => c,
    };
    encode(sink, kind, codec, items)?;
    Ok(codec)
}

/// Reader of a numeric column of 1- to 64-bit values, stepped one item at a time.
#[derive(Clone, Copy, Debug)]
pub struct NumericReader<'i> {
    rd: BufReader<'i>,
    kind: NumericKind,
    codec: NumericCodec,
    prev: Option<u64>,
    prev_delta: i64,
    /// The constant coder's residual, once read.
    constant: Option<u64>,
    /// Residuals left in the current packed block, and their width.
    block_left: u8,
    block_width: u8,
}

impl<'i> NumericReader<'i> {
    /// Reads the header at `rd`'s position: the escape predictor `11` is `MalformedSeries`.
    pub(crate) fn new(mut rd: BufReader<'i>, kind: NumericKind) -> Result<Self, Error> {
        let predictor = match rd.read_un32(2)? {
            0 => Predictor::Raw,
            1 => Predictor::Delta,
            2 => Predictor::DeltaOfDelta,
            _ => return Err(Error::MalformedSeries),
        };
        let coder = match rd.read_un32(2)? {
            0 => Coder::Prefix,
            1 => Coder::Packed,
            2 => Coder::Constant,
            _ => Coder::Plain,
        };
        Ok(NumericReader {
            rd,
            kind,
            codec: NumericCodec::new(predictor, coder),
            prev: None,
            prev_delta: 0,
            constant: None,
            block_left: 0,
            block_width: 0,
        })
    }

    #[cfg(any(test, fuzzing))]
    pub(crate) fn codec(&self) -> NumericCodec {
        self.codec
    }

    /// The next item's value bits (zero-extended from the width; sign-extend signed ones), `restart` as the writer
    /// had it. Reads at most `W` + 9 bits.
    pub(crate) fn next(&mut self, restart: bool) -> Result<u64, Error> {
        let w = self.kind.width;
        let predictor = self.codec.predictor;
        if predictor != Predictor::Raw && (restart || self.prev.is_none()) {
            let bits = self.rd.read_un64(w)?;
            self.prev = Some(bits);
            self.prev_delta = 0;
            self.block_left = 0;
            return Ok(bits);
        }
        let z = match self.codec.coder {
            Coder::Prefix => get_delta(&mut self.rd, w)?,
            Coder::Plain => self.rd.read_un64(w)?,
            Coder::Constant => match self.constant {
                Some(z) => z,
                None => {
                    let z = get_delta(&mut self.rd, w)?;
                    self.constant = Some(z);
                    z
                }
            },
            Coder::Packed => {
                if self.block_left == 0 {
                    let block_width = self.rd.read_un32(block_width_bits(w))? as u8;
                    if block_width > w {
                        return Err(Error::MalformedSeries);
                    }
                    self.block_width = block_width;
                    self.block_left = PACKED_BLOCK;
                }
                self.block_left -= 1;
                self.rd.read_un64(self.block_width)?
            }
        };
        let Some(prev) = self.prev.filter(|_| predictor != Predictor::Raw) else {
            // raw: z is below 2^W, so is its unzigzag at W
            return Ok(if self.kind.signed {
                (unzigzag(z) as u64) & mask(w)
            } else {
                z
            });
        };
        let coded = unzigzag(z);
        let delta = if predictor == Predictor::DeltaOfDelta {
            sign_extend((coded.wrapping_add(self.prev_delta) as u64) & mask(w), w)
        } else {
            coded
        };
        let bits = prev.wrapping_add(delta as u64) & mask(w);
        self.prev = Some(bits);
        self.prev_delta = delta;
        Ok(bits)
    }

    /// The reader positioned after the bits read so far: after the last item, where the next column starts.
    pub(crate) fn rest(&self) -> BufReader<'i> {
        self.rd
    }
}

/// Zigzag at 128 bits.
const fn zigzag128(v: i128) -> u128 {
    ((v << 1) ^ (v >> 127)) as u128
}

const fn unzigzag128(z: u128) -> i128 {
    ((z >> 1) as i128) ^ -((z & 1) as i128)
}

/// Writes a numeric column of `u128` / `i128` values (two's complement bits for signed ones): header raw + plain,
/// each value (zigzag for signed) at 128 bits. No other codec exists for them.
pub(crate) fn write_numeric128<I>(
    sink: &mut impl BitSink,
    signed: bool,
    items: I,
) -> Result<(), Error>
where
    I: Iterator<Item = u128>,
{
    sink.put_bits(4, 0b0011)?;
    for v in items {
        let z = if signed { zigzag128(v as i128) } else { v };
        sink.put_bits(64, (z >> 64) as u64)?;
        sink.put_bits(64, z as u64)?;
    }
    Ok(())
}

/// Reader of a [write_numeric128] column.
#[derive(Clone, Copy, Debug)]
pub struct Numeric128Reader<'i> {
    rd: BufReader<'i>,
    signed: bool,
}

impl<'i> Numeric128Reader<'i> {
    /// Reads the header: anything but raw + plain is `MalformedSeries`.
    pub(crate) fn new(mut rd: BufReader<'i>, signed: bool) -> Result<Self, Error> {
        if rd.read_un32(4)? != 0b0011 {
            return Err(Error::MalformedSeries);
        }
        Ok(Numeric128Reader { rd, signed })
    }

    /// The next value's bits (two's complement for signed ones).
    pub(crate) fn next(&mut self) -> Result<u128, Error> {
        let z = ((self.rd.read_un64(64)? as u128) << 64) | self.rd.read_un64(64)? as u128;
        Ok(if self.signed {
            unzigzag128(z) as u128
        } else {
            z
        })
    }

    pub(crate) fn rest(&self) -> BufReader<'i> {
        self.rd
    }
}

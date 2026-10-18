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

use super::{Coder, NumericCodec, PACKED_BLOCK, Predictor};
use crate::series::{BitSink, delta_bits, get_delta, mask, put_delta, sign_extend, unzigzag};
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

/// The machine word a numeric column is written in: `u32` for columns up to 32 bits, so that a 32-bit MCU does
/// one instruction where `u64` takes several, `u64` up to 64. The leaf type picks it, so a firmware links the
/// writer once per word it uses.
pub(crate) trait Word:
    Copy + Eq + core::ops::BitOr<Output = Self> + core::ops::BitOrAssign
{
    const BITS: u32;
    const ZERO: Self;
    fn shl(self, n: u32) -> Self;
    fn shr(self, n: u32) -> Self;
    fn wrapping_sub(self, other: Self) -> Self;
    /// Zigzag of the word as a two's complement number.
    fn zigzag(self) -> Self;
    /// Bits the value needs: 0 for 0.
    fn needs(self) -> u8;
    fn to_u64(self) -> u64;
}

macro_rules! word {
    ($ty:ty, $signed:ty) => {
        impl Word for $ty {
            const BITS: u32 = <$ty>::BITS;
            const ZERO: Self = 0;

            #[inline(always)]
            fn shl(self, n: u32) -> Self {
                self << n
            }

            #[inline(always)]
            fn shr(self, n: u32) -> Self {
                self >> n
            }

            #[inline(always)]
            fn wrapping_sub(self, other: Self) -> Self {
                <$ty>::wrapping_sub(self, other)
            }

            #[inline(always)]
            fn zigzag(self) -> Self {
                let v = self as $signed;
                ((v << 1) ^ (v >> (<$ty>::BITS - 1))) as $ty
            }

            #[inline(always)]
            fn needs(self) -> u8 {
                (<$ty>::BITS - self.leading_zeros()) as u8
            }

            #[inline(always)]
            fn to_u64(self) -> u64 {
                self as u64
            }
        }
    };
}

word!(u32, i32);
word!(u64, i64);

/// What the predictor makes of one item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step<W> {
    /// A segment start, the value's bits at full width.
    Start(W),
    /// A residual, below `2^W`.
    Residual(W),
}

/// The predictor, one item at a time: what each value becomes, a segment start or a residual.
///
/// Works on the values shifted to the top of the word (left-aligned): a difference of two of them is the
/// difference wrapped at W and sign-extended, shifted up the same, and the zigzag of that shifted back down is the
/// zigzag at W. So no masking or sign extension per item.
#[derive(Clone, Copy)]
struct Predict<W> {
    /// Word bits - W.
    shift: u32,
    signed: bool,
    predictor: Predictor,
    /// The previous value and difference, left-aligned.
    prev: Option<W>,
    prev_delta: W,
}

impl<W: Word> Predict<W> {
    fn new(kind: NumericKind, predictor: Predictor) -> Self {
        assert!(
            kind.width as u32 <= W::BITS,
            "numeric column wider than its word"
        );
        Predict {
            shift: W::BITS - kind.width as u32,
            signed: kind.signed,
            predictor,
            prev: None,
            prev_delta: W::ZERO,
        }
    }

    #[inline(always)]
    fn step(&mut self, value: W, restart: bool) -> Step<W> {
        let s = self.shift;
        let v = value.shl(s);
        if self.predictor == Predictor::Raw {
            return Step::Residual(if self.signed {
                v.zigzag().shr(s)
            } else {
                v.shr(s)
            });
        }
        let step = match self.prev {
            Some(prev) if !restart => {
                let delta = v.wrapping_sub(prev);
                let coded = if self.predictor == Predictor::DeltaOfDelta {
                    delta.wrapping_sub(self.prev_delta)
                } else {
                    delta
                };
                self.prev_delta = delta;
                Step::Residual(coded.zigzag().shr(s))
            }
            _ => {
                self.prev_delta = W::ZERO;
                Step::Start(v.shr(s))
            }
        };
        self.prev = Some(v);
        step
    }
}

/// Writes a column's values with the packed coder, one item at a time: segment starts at full width, residuals in
/// blocks of up to [PACKED_BLOCK] (a block ends early at a segment start or the column end), each block its width
/// and the residuals at that width. A block's residuals are kept until its width is known.
///
/// The column writers are structs stepped by a loop over the items, not functions over an iterator: the loop is
/// the only code each column's item iterator (a type of its own) gets, the writer is linked once per word and sink.
struct PackedWriter<W> {
    predict: Predict<W>,
    w: u8,
    block: [W; PACKED_BLOCK as usize],
    len: usize,
    bits: W,
}

impl<W: Word> PackedWriter<W> {
    fn new(kind: NumericKind, predictor: Predictor) -> Self {
        PackedWriter {
            predict: Predict::new(kind, predictor),
            w: kind.width,
            block: [W::ZERO; PACKED_BLOCK as usize],
            len: 0,
            bits: W::ZERO,
        }
    }

    #[inline(never)]
    fn push<S: BitSink>(&mut self, sink: &mut S, value: W, restart: bool) -> Result<(), Error> {
        match self.predict.step(value, restart) {
            Step::Start(bits) => {
                self.finish(sink)?;
                sink.put_bits(self.w, bits.to_u64())
            }
            Step::Residual(z) => {
                self.block[self.len] = z;
                self.bits |= z;
                self.len += 1;
                if self.len == PACKED_BLOCK as usize {
                    self.finish(sink)?;
                }
                Ok(())
            }
        }
    }

    /// Writes the open block, if any.
    fn finish<S: BitSink>(&mut self, sink: &mut S) -> Result<(), Error> {
        if self.len == 0 {
            return Ok(());
        }
        let width = self.bits.needs();
        sink.put_bits(block_width_bits(self.w), width as u64)?;
        for &z in &self.block[..self.len] {
            sink.put_bits(width, z.to_u64())?;
        }
        self.len = 0;
        self.bits = W::ZERO;
        Ok(())
    }
}

/// Writes a column's values with the prefix, constant or plain coder, one item at a time (see [PackedWriter]).
struct ResidualWriter<W> {
    predict: Predict<W>,
    w: u8,
    coder: Coder,
    constant_written: bool,
}

impl<W: Word> ResidualWriter<W> {
    fn new(kind: NumericKind, codec: NumericCodec) -> Self {
        ResidualWriter {
            predict: Predict::new(kind, codec.predictor),
            w: kind.width,
            coder: codec.coder,
            constant_written: false,
        }
    }

    #[inline(never)]
    fn push<S: BitSink>(&mut self, sink: &mut S, value: W, restart: bool) -> Result<(), Error> {
        let z = match self.predict.step(value, restart) {
            Step::Start(bits) => return sink.put_bits(self.w, bits.to_u64()),
            Step::Residual(z) => z.to_u64(),
        };
        match self.coder {
            Coder::Prefix => put_delta(sink, z, self.w),
            Coder::Constant if self.constant_written => Ok(()),
            Coder::Constant => {
                self.constant_written = true;
                put_delta(sink, z, self.w)
            }
            _ => sink.put_bits(self.w, z),
        }
    }
}

/// Bits a column takes under one predictor with each coder, in [CODERS] order (header included), counted one item
/// at a time (see [PackedWriter]): what the writers write, `None` for constant when the residuals differ.
struct Sizes<W> {
    predict: Predict<W>,
    w: u8,
    starts: usize,
    residuals: usize,
    prefix: usize,
    packed: usize,
    first: Option<W>,
    constant: bool,
    block_len: usize,
    block_bits: W,
}

impl<W: Word> Sizes<W> {
    fn new(kind: NumericKind, predictor: Predictor) -> Self {
        Sizes {
            predict: Predict::new(kind, predictor),
            w: kind.width,
            starts: 0,
            residuals: 0,
            prefix: 0,
            packed: 0,
            first: None,
            constant: true,
            block_len: 0,
            block_bits: W::ZERO,
        }
    }

    #[inline(never)]
    fn push(&mut self, value: W, restart: bool) {
        match self.predict.step(value, restart) {
            Step::Start(_) => {
                self.starts += 1;
                self.end_block();
            }
            Step::Residual(z) => {
                self.residuals += 1;
                self.prefix += delta_bits(z.needs(), self.w);
                match self.first {
                    None => self.first = Some(z),
                    Some(f) => self.constant &= f == z,
                }
                self.block_bits |= z;
                self.block_len += 1;
                if self.block_len == PACKED_BLOCK as usize {
                    self.end_block();
                }
            }
        }
    }

    fn end_block(&mut self) {
        if self.block_len > 0 {
            self.packed += block_width_bits(self.w) as usize
                + self.block_len * self.block_bits.needs() as usize;
            self.block_len = 0;
            self.block_bits = W::ZERO;
        }
    }

    fn finish(mut self) -> [Option<usize>; 4] {
        self.end_block();
        let head = 4 + self.starts * self.w as usize;
        let w = self.w;
        [
            Some(head + self.prefix),
            Some(head + self.packed),
            self.constant
                .then(|| head + self.first.map_or(0, |z| delta_bits(z.needs(), w))),
            Some(head + self.residuals * w as usize),
        ]
    }
}

/// [Sizes] of a column's items under `predictor`.
fn coder_bits<I, W>(kind: NumericKind, predictor: Predictor, items: I) -> [Option<usize>; 4]
where
    I: Iterator<Item = (W, bool)>,
    W: Word,
{
    let mut sizes = Sizes::new(kind, predictor);
    for (value, restart) in items {
        sizes.push(value, restart);
    }
    sizes.finish()
}

/// Writes the column with `codec`, header included, which the caller checked fits (constant).
#[inline(always)]
fn encode<I, W>(
    sink: &mut impl BitSink,
    kind: NumericKind,
    codec: NumericCodec,
    items: I,
) -> Result<(), Error>
where
    I: Iterator<Item = (W, bool)>,
    W: Word,
{
    sink.put_bits(4, ((codec.predictor as u64) << 2) | codec.coder as u64)?;
    if codec.coder == Coder::Packed {
        let mut writer = PackedWriter::new(kind, codec.predictor);
        for (value, restart) in items {
            writer.push(sink, value, restart)?;
        }
        writer.finish(sink)
    } else {
        let mut writer = ResidualWriter::new(kind, codec);
        for (value, restart) in items {
            writer.push(sink, value, restart)?;
        }
        Ok(())
    }
}

/// Bits the column takes with `codec` (header included), `None` if the codec can't encode it: counted by writing
/// it into a [BitCounter](super::BitCounter), what [coder_bits] is checked against.
#[cfg(any(test, fuzzing))]
pub(crate) fn numeric_bits<I, W>(kind: NumericKind, codec: NumericCodec, items: I) -> Option<usize>
where
    I: Iterator<Item = (W, bool)> + Clone,
    W: Word,
{
    let mut counter = super::BitCounter::new();
    if codec.coder == Coder::Constant {
        // the residuals must all be the same: what a writer puts in more than one value would read back wrong
        let mut first = None;
        let mut predict = Predict::new(kind, codec.predictor);
        for (value, restart) in items.clone() {
            if let Step::Residual(z) = predict.step(value, restart) {
                if first.is_some_and(|f| f != z) {
                    return None;
                }
                first = Some(z);
            }
        }
    }
    encode(&mut counter, kind, codec, items).ok()?;
    Some(counter.bits())
}

/// [coder_bits] of `codec`, for the checks against [numeric_bits].
#[cfg(any(test, fuzzing))]
pub(crate) fn counted_bits<I, W>(kind: NumericKind, codec: NumericCodec, items: I) -> Option<usize>
where
    I: Iterator<Item = (W, bool)>,
    W: Word,
{
    coder_bits(kind, codec.predictor, items)[codec.coder as usize]
}

/// The codec with the fewest bits for the column, trying predictors raw, delta, delta of delta, each with coders
/// prefix, packed, constant, plain; the first of equal ones wins.
pub(crate) fn smallest_numeric<I, W>(kind: NumericKind, items: I) -> NumericCodec
where
    I: Iterator<Item = (W, bool)> + Clone,
    W: Word,
{
    let mut best = (NumericCodec::new(Predictor::Raw, Coder::Plain), usize::MAX);
    for predictor in PREDICTORS {
        let sizes = coder_bits(kind, predictor, items.clone());
        for (coder, bits) in CODERS.into_iter().zip(sizes) {
            if let Some(bits) = bits
                && bits < best.1
            {
                best = (NumericCodec::new(predictor, coder), bits);
            }
        }
    }
    best.0
}

/// Writes a numeric column of 1- to 64-bit values: `items` are each item's value bits in a [Word] at least the
/// width wide (bits above the width are ignored, signed values sign-extended or not) and whether it restarts the
/// column. `codec` `None` picks the smallest; a fixed constant that doesn't fit falls back to packed with the same
/// predictor. Returns the codec written. Writes nothing but the header for no items: a `Series` doesn't write
/// empty columns at all.
///
/// Inlined, so that a `codec` known at compile time ([SeriesPolicy](super::SeriesPolicy) types) leaves the
/// trials and the other coders' writers out of the firmware.
#[inline(always)]
pub(crate) fn write_numeric<I, W>(
    sink: &mut impl BitSink,
    kind: NumericKind,
    items: I,
    codec: Option<NumericCodec>,
) -> Result<NumericCodec, Error>
where
    I: Iterator<Item = (W, bool)> + Clone,
    W: Word,
{
    let codec = match codec {
        None => smallest_numeric(kind, items.clone()),
        Some(c)
            if c.coder == Coder::Constant
                && coder_bits(kind, c.predictor, items.clone())[2].is_none() =>
        {
            NumericCodec::new(c.predictor, Coder::Packed)
        }
        Some(c) => c,
    };
    encode(sink, kind, codec, items)?;
    Ok(codec)
}

/// Writes a numeric column of `values` without restarts, the bits [write_numeric] writes for
/// `values.map(|v| (word(v), false))`, from an array as a [SeriesStore](crate::series::SeriesStore) keeps it:
/// delta packed is one pass with the block widths and bits packed in a word; every other codec goes through
/// [write_numeric].
#[inline(always)]
pub(crate) fn write_numeric_slice<T, W>(
    sink: &mut impl BitSink,
    kind: NumericKind,
    values: &[T],
    word: impl Fn(&T) -> W + Copy,
    codec: Option<NumericCodec>,
) -> Result<NumericCodec, Error>
where
    W: Word,
{
    match codec {
        Some(c) if c == NumericCodec::new(Predictor::Delta, Coder::Packed) => {
            delta_packed_slice(sink, kind, values, word)?;
            Ok(c)
        }
        _ => write_numeric(sink, kind, values.iter().map(|v| (word(v), false)), codec),
    }
}

/// Delta packed without restarts, one pass: the first value at the width, then blocks of [PACKED_BLOCK]
/// residuals, each its width field and the residuals packed into a 64-bit word before they go to the sink. The
/// same bits as [PackedWriter].
#[inline(never)]
fn delta_packed_slice<T, W: Word>(
    sink: &mut impl BitSink,
    kind: NumericKind,
    values: &[T],
    word: impl Fn(&T) -> W,
) -> Result<(), Error> {
    sink.put_bits(4, ((Predictor::Delta as u64) << 2) | Coder::Packed as u64)?;
    let Some((first, rest)) = values.split_first() else {
        return Ok(());
    };
    let shift = W::BITS - kind.width as u32;
    let mut prev = word(first).shl(shift);
    sink.put_bits(kind.width, prev.shr(shift).to_u64())?;
    let width_bits = block_width_bits(kind.width);
    for block in rest.chunks(PACKED_BLOCK as usize) {
        let mut z = [W::ZERO; PACKED_BLOCK as usize];
        let mut any = W::ZERO;
        for (z, v) in z.iter_mut().zip(block) {
            let v = word(v).shl(shift);
            *z = v.wrapping_sub(prev).zigzag().shr(shift);
            any |= *z;
            prev = v;
        }
        let width = any.needs();
        sink.put_bits(width_bits, width as u64)?;
        if width == 0 {
            continue;
        }
        if width > 32 {
            for z in &z[..block.len()] {
                sink.put_bits(width, z.to_u64())?;
            }
            continue;
        }
        // up to two residuals of up to 32 bits per 64-bit word
        let (mut acc, mut n) = (0u64, 0u8);
        for z in &z[..block.len()] {
            if n + width > 64 {
                sink.put_bits(n, acc)?;
                (acc, n) = (0, 0);
            }
            acc = acc << width | z.to_u64();
            n += width;
        }
        sink.put_bits(n, acc)?;
    }
    Ok(())
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

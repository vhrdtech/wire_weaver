//! Round trips and reads of random bytes for every column codec, shared by the unit tests and the `series-columns`
//! fuzz target (`cargo fuzz` builds with `--cfg fuzzing`, which makes this module public): a mismatch or a panic
//! is a bug. Writers get a scratch buffer from the caller, big enough for the items (a writer running out of it
//! panics here).

use super::float::{FloatReader, float_bits, write_float};
use super::numeric::{
    Numeric128Reader, NumericKind, NumericReader, counted_bits, numeric_bits, write_numeric,
    write_numeric128,
};
use super::run::{Opaque, RunKind, RunReader, write_opaque_run, write_run};
pub use super::{Coder, FloatCodec, NumericCodec, Predictor};
use crate::series::{XorBits, mask};
use crate::{BufReader, BufWriter, Error};

/// Bits written so far.
fn written(wr: &BufWriter<'_>) -> usize {
    let (byte, bit) = wr.pos();
    byte * 8 + 7 - bit as usize
}

/// Bits read so far.
fn read(rd: &BufReader<'_>) -> usize {
    let (byte, bit) = rd.pos();
    byte * 8 + 7 - bit as usize
}

/// Every numeric codec: the writer's trials in their order, then aligned plain (never smaller than raw plain, not
/// tried).
pub fn numeric_codecs() -> impl Iterator<Item = NumericCodec> {
    [Predictor::Raw, Predictor::Delta, Predictor::DeltaOfDelta]
        .into_iter()
        .flat_map(|p| {
            [Coder::Prefix, Coder::Packed, Coder::Constant, Coder::Plain]
                .into_iter()
                .map(move |c| NumericCodec::new(p, c))
        })
        .chain([NumericCodec::ALIGNED_PLAIN])
}

/// A numeric column of `width` (1..=64) bits: written with `codec` (`None`: the smallest), it reads back the same
/// values with only the bytes it takes, its bit count is what [numeric_bits] says, and the smallest is no bigger
/// than any other codec. Returns the codec written and its bits.
pub fn numeric(
    buf: &mut [u8],
    width: u8,
    signed: bool,
    items: &[(u64, bool)],
    codec: Option<NumericCodec>,
) -> (NumericCodec, usize) {
    let kind = NumericKind::new(width, signed);
    let iter = items.iter().copied();
    let mut wr = BufWriter::new(buf);
    let written_codec = write_numeric(&mut wr, kind, iter.clone(), codec).expect("write");
    let bits = written(&wr);
    let buf = wr.deinit();
    if width <= 32 {
        // the 32-bit word the leaves up to 32 bits use writes the same bits
        let mut buf32 = [0u8; 2048];
        let mut wr = BufWriter::new(&mut buf32);
        let items32 = items.iter().map(|&(v, r)| (v as u32, r));
        assert_eq!(
            write_numeric(&mut wr, kind, items32, codec).expect("write u32"),
            written_codec
        );
        assert_eq!(written(&wr), bits);
        let n = bits.div_ceil(8);
        assert_eq!(wr.deinit()[..n], buf[..n]);
    }
    if let Some(c) = codec
        && c.coder != Coder::Constant
    {
        assert_eq!(written_codec, c);
    }
    assert_eq!(numeric_bits(kind, written_codec, iter.clone()), Some(bits));
    for c in numeric_codecs() {
        let b = numeric_bits(kind, c, iter.clone());
        // the one-pass sizes the smallest is picked by are the bits each codec writes
        assert_eq!(counted_bits(kind, c, iter.clone()), b, "{c:?}");
        if codec.is_none()
            && let Some(b) = b
        {
            assert!(bits <= b, "{written_codec:?} {bits} bits, {c:?} {b}");
        }
    }
    let mut rd =
        NumericReader::new(BufReader::new(&buf[..bits.div_ceil(8)]), kind).expect("header");
    assert_eq!(rd.codec(), written_codec);
    for &(value, restart) in items {
        assert_eq!(rd.next(restart), Ok(value & mask(width)));
    }
    assert_eq!(read(&rd.rest()), bits);
    (written_codec, bits)
}

/// A 128-bit numeric column round trip.
pub fn numeric128(buf: &mut [u8], signed: bool, values: &[u128]) {
    let mut wr = BufWriter::new(buf);
    write_numeric128(&mut wr, signed, values.iter().copied()).expect("write");
    let bits = written(&wr);
    assert_eq!(bits, 4 + 128 * values.len());
    let buf = wr.deinit();
    let mut rd =
        Numeric128Reader::new(BufReader::new(&buf[..bits.div_ceil(8)]), signed).expect("header");
    for &v in values {
        assert_eq!(rd.next(), Ok(v));
    }
    assert_eq!(read(&rd.rest()), bits);
}

/// A float column of `T` round trip, values as bit patterns (compared bit by bit, so NaNs too).
pub fn float<T: XorBits>(
    buf: &mut [u8],
    items: &[(u64, bool)],
    codec: Option<FloatCodec>,
) -> (FloatCodec, usize) {
    let iter = items
        .iter()
        .map(|&(bits, restart)| (T::from_bits(bits & mask(T::BITS)), restart));
    let mut wr = BufWriter::new(buf);
    let written_codec = write_float(&mut wr, iter.clone(), codec).expect("write");
    let bits = written(&wr);
    let buf = wr.deinit();
    if let Some(c) = codec {
        assert_eq!(written_codec, c);
    }
    assert_eq!(float_bits(written_codec, iter.clone()), bits);
    if codec.is_none() {
        assert!(bits <= float_bits(FloatCodec::Plain, iter.clone()));
        assert!(bits <= float_bits(FloatCodec::Xor, iter.clone()));
        assert!(bits <= float_bits(FloatCodec::Aligned, iter.clone()));
    }
    let mut rd = FloatReader::<T>::new(BufReader::new(&buf[..bits.div_ceil(8)])).expect("header");
    assert_eq!(rd.codec(), written_codec);
    for &(value, restart) in items {
        assert_eq!(rd.next(restart).map(T::to_bits), Ok(value & mask(T::BITS)));
    }
    assert_eq!(read(&rd.rest()), bits);
    (written_codec, bits)
}

/// Writes `offset` (0..8) one bits ahead of a column, so that byte alignment inside it is exercised, and returns
/// a reader positioned after them.
fn skip_offset<'i>(buf: &'i [u8], offset: u8) -> BufReader<'i> {
    let mut rd = BufReader::new(buf);
    assert_eq!(rd.read_un32(offset), Ok(mask(offset) as u32));
    rd
}

/// A run of tags or bools (`kind` 0: tag, 1: `Option` tag, 2: bool; two-valued values taken `& 1`) after
/// `offset % 8` bits: reads back the same values, tells changes right, and `finish` stops where stepping does.
/// Returns the bits of the run.
pub fn run(buf: &mut [u8], kind: u8, values: &[u32], offset: u8) -> usize {
    let kind = [RunKind::Tag, RunKind::OptionTag, RunKind::Bool][kind as usize % 3];
    let offset = offset % 8;
    let m = values.len();
    let values = || {
        values
            .iter()
            .map(move |&v| if kind == RunKind::Tag { v } else { v & 1 })
    };
    let mut wr = BufWriter::new(buf);
    wr.write_un32(offset, u32::MAX).expect("offset");
    write_run(&mut wr, kind, values()).expect("write");
    let end = written(&wr);
    let buf = wr.deinit();
    if m == 0 {
        assert_eq!(end, offset as usize);
        return 0;
    }
    let buf = &buf[..end.div_ceil(8)];
    let start = skip_offset(buf, offset);
    let mut rd = RunReader::new(start, kind, m).expect("first");
    let mut prev = None;
    for v in values() {
        assert_eq!(rd.changes_next(), prev.is_some_and(|p| p != v));
        assert_eq!(rd.next(), Ok(v));
        prev = Some(v);
    }
    assert_eq!(rd.next(), Err(Error::MalformedSeries));
    assert_eq!(read(&rd.finish().expect("finish")), end);
    let rd = RunReader::new(start, kind, m).expect("first");
    assert_eq!(read(&rd.finish().expect("finish")), end);
    end - offset as usize
}

/// An opaque run after `offset % 8` bits, as [run].
pub fn opaque(buf: &mut [u8], values: &[&[u8]], offset: u8) -> usize {
    let offset = offset % 8;
    let mut wr = BufWriter::new(buf);
    wr.write_un32(offset, u32::MAX).expect("offset");
    write_opaque_run(&mut wr, values.iter().copied()).expect("write");
    let end = written(&wr);
    let buf = wr.deinit();
    if values.is_empty() {
        assert_eq!(end, offset as usize);
        return 0;
    }
    let buf = &buf[..end.div_ceil(8)];
    let start = skip_offset(buf, offset);
    let mut rd = RunReader::new(start, Opaque, values.len()).expect("first");
    for &v in values {
        assert_eq!(rd.next(), Ok(v));
    }
    assert_eq!(read(&rd.finish().expect("finish")), end);
    let rd = RunReader::new(start, Opaque, values.len()).expect("first");
    assert_eq!(read(&rd.finish().expect("finish")), end);
    end - offset as usize
}

/// Reads `bytes` as one column (`selector` picks numeric of any width and signedness, 128-bit, `f32` / `f64`,
/// tag / bool / `Option` / opaque runs) over `restarts.len()` items: any outcome but a panic, an endless loop or
/// a read past `bytes` is fine. Returns whether every item read.
pub fn garbage(bytes: &[u8], selector: u8, restarts: &[bool]) -> bool {
    let rd = BufReader::new(bytes);
    let m = restarts.len();
    let result: Result<(), Error> = (|| {
        match selector % 9 {
            0..=1 => {
                let mut c = NumericReader::new(
                    rd,
                    NumericKind::new(selector / 2 % 64 + 1, selector & 1 == 1),
                )?;
                for &r in restarts {
                    c.next(r)?;
                }
            }
            2 => {
                let mut c = Numeric128Reader::new(rd, selector & 0x10 != 0)?;
                for _ in 0..m {
                    c.next()?;
                }
            }
            3 => {
                let mut c = FloatReader::<f32>::new(rd)?;
                for &r in restarts {
                    c.next(r)?;
                }
            }
            4 => {
                let mut c = FloatReader::<f64>::new(rd)?;
                for &r in restarts {
                    c.next(r)?;
                }
            }
            5..=7 => {
                let kind =
                    [RunKind::Tag, RunKind::OptionTag, RunKind::Bool][selector as usize % 9 - 5];
                let mut c = RunReader::new(rd, kind, m)?;
                // the first half stepped, the rest skipped
                for _ in 0..m / 2 {
                    c.next()?;
                }
                c.finish()?;
            }
            _ => {
                let mut c = RunReader::new(rd, Opaque, m)?;
                for _ in 0..m / 2 {
                    c.next()?;
                }
                c.finish()?;
            }
        }
        Ok(())
    })();
    result.is_ok()
}

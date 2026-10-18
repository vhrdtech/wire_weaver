//! Float column: `f32` / `f64`.
//!
//! ```text
//! codec:      4 bits   0000 plain (each value at full width), 0001 XOR (the SW-32 Gorilla XOR code),
//!                      0010 aligned plain (zero bits to a byte, each value little-endian in 4 or 8 bytes),
//!                      others reserved
//! ```
//!
//! XOR: the first value of each segment at full width, then the SW-32 code against the previous value; a segment
//! start also forgets the leading / trailing zero window. Plain ignores restarts.

use super::{BitCounter, FloatCodec};
use crate::series::xor_float::XorCodec;
use crate::series::{BitSink, Codec, XorBits};
use crate::{BufReader, Error};

fn encode<T, I>(sink: &mut impl BitSink, codec: FloatCodec, items: I) -> Result<(), Error>
where
    T: XorBits,
    I: Iterator<Item = (T, bool)>,
{
    sink.put_bits(4, codec as u64)?;
    if codec == FloatCodec::Aligned {
        sink.align_byte();
        let bytes = T::BITS as usize / 8;
        for (value, _) in items {
            sink.put_bytes(&value.to_bits().to_le_bytes()[..bytes])?;
        }
        return Ok(());
    }
    let mut xor = XorCodec::<T>::default();
    for (value, restart) in items {
        match codec {
            FloatCodec::Plain => sink.put_bits(T::BITS, value.to_bits())?,
            FloatCodec::Xor => {
                if restart {
                    xor = XorCodec::default();
                }
                xor.encode(value, sink)?;
            }
            FloatCodec::Aligned => unreachable!("written above"),
        }
    }
    Ok(())
}

/// Bits the column takes with `codec`, header included.
pub(crate) fn float_bits<T, I>(codec: FloatCodec, items: I) -> usize
where
    T: XorBits,
    I: Iterator<Item = (T, bool)>,
{
    let mut counter = BitCounter::new();
    // a counter never fails
    let _ = encode(&mut counter, codec, items);
    counter.bits()
}

/// Writes a float column: `items` are each item's value and whether it restarts the column, `codec` `None`
/// picks the smaller of plain and XOR (plain on a tie). Returns the codec written.
pub(crate) fn write_float<T, I>(
    sink: &mut impl BitSink,
    items: I,
    codec: Option<FloatCodec>,
) -> Result<FloatCodec, Error>
where
    T: XorBits,
    I: Iterator<Item = (T, bool)> + Clone,
{
    let codec = codec.unwrap_or_else(|| {
        // plain is 4 + n x W bits, known without a pass
        let n = items.clone().count();
        let plain = 4 + n * T::BITS as usize;
        if float_bits(FloatCodec::Xor, items.clone()) < plain {
            FloatCodec::Xor
        } else {
            FloatCodec::Plain
        }
    });
    encode(sink, codec, items)?;
    Ok(codec)
}

/// Writes a float column of `values` without restarts, the bits [write_float] writes for them: aligned plain is
/// one copy on a little-endian target (a float's memory is its bits).
#[inline(always)]
pub(crate) fn write_float_slice<T: XorBits>(
    sink: &mut impl BitSink,
    values: &[T],
    codec: Option<FloatCodec>,
) -> Result<FloatCodec, Error> {
    if codec == Some(FloatCodec::Aligned) && cfg!(target_endian = "little") {
        sink.put_bits(4, FloatCodec::Aligned as u64)?;
        sink.align_byte();
        // SAFETY: `T` is `f32` or `f64` (`XorBits`), plain bytes, and `u8` has no alignment
        let bytes = unsafe {
            core::slice::from_raw_parts(
                values.as_ptr().cast::<u8>(),
                core::mem::size_of_val(values),
            )
        };
        sink.put_bytes(bytes)?;
        return Ok(FloatCodec::Aligned);
    }
    write_float(sink, values.iter().map(|v| (*v, false)), codec)
}

/// Reader of a float column, stepped one item at a time.
pub struct FloatReader<'i, T> {
    rd: BufReader<'i>,
    codec: FloatCodec,
    xor: XorCodec<T>,
}

impl<T> Clone for FloatReader<'_, T> {
    fn clone(&self) -> Self {
        FloatReader {
            rd: self.rd,
            codec: self.codec,
            xor: self.xor,
        }
    }
}

impl<'i, T: XorBits> FloatReader<'i, T> {
    /// Reads the codec field at `rd`'s position: reserved codes are `MalformedSeries`.
    pub(crate) fn new(mut rd: BufReader<'i>) -> Result<Self, Error> {
        let codec = match rd.read_un32(4)? {
            0 => FloatCodec::Plain,
            1 => FloatCodec::Xor,
            2 => {
                rd.align_byte();
                FloatCodec::Aligned
            }
            _ => return Err(Error::MalformedSeries),
        };
        Ok(FloatReader {
            rd,
            codec,
            xor: XorCodec::default(),
        })
    }

    #[cfg(any(test, fuzzing))]
    pub(crate) fn codec(&self) -> FloatCodec {
        self.codec
    }

    /// The next item's value, `restart` as the writer had it. Reads at most `W` + 14 bits.
    pub(crate) fn next(&mut self, restart: bool) -> Result<T, Error> {
        match self.codec {
            FloatCodec::Plain => Ok(T::from_bits(self.rd.read_un64(T::BITS)?)),
            FloatCodec::Aligned => {
                let mut le = [0u8; 8];
                let bytes = T::BITS as usize / 8;
                le[..bytes].copy_from_slice(self.rd.read_raw_slice(bytes)?);
                Ok(T::from_bits(u64::from_le_bytes(le)))
            }
            FloatCodec::Xor => {
                if restart {
                    self.xor = XorCodec::default();
                }
                self.xor.decode(&mut self.rd)
            }
        }
    }

    /// The reader positioned after the bits read so far.
    pub(crate) fn rest(&self) -> BufReader<'i> {
        self.rd
    }
}

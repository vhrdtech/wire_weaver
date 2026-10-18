//! [XorBits] / [XorCodec]: the SW-32 Gorilla XOR code for `f32` / `f64`, against the previous value. Internal to
//! [Series](super::Series)' float columns; `dynamic::read_float` and the `columns::check` fuzz/test helper are
//! generic over [XorBits] too.

use super::{BitSink, Codec};
use crate::{BufReader, Error};

/// Floats a float column takes: `f32` and `f64`, handled as their bit patterns, so NaN payloads, `-0.0` and
/// infinities come back exactly.
pub trait XorBits: Copy + sealed::Sealed {
    const BITS: u8;
    /// Bits of the leading zero count in a `11` code: 5 for `f32`, 6 for `f64`.
    const LEAD_BITS: u8;
    fn to_bits(self) -> u64;
    fn from_bits(bits: u64) -> Self;
}

mod sealed {
    pub trait Sealed {}
}

impl sealed::Sealed for f32 {}
impl XorBits for f32 {
    const BITS: u8 = 32;
    const LEAD_BITS: u8 = 5;
    #[inline]
    fn to_bits(self) -> u64 {
        f32::to_bits(self) as u64
    }
    #[inline]
    fn from_bits(bits: u64) -> Self {
        f32::from_bits(bits as u32)
    }
}

impl sealed::Sealed for f64 {}
impl XorBits for f64 {
    const BITS: u8 = 64;
    const LEAD_BITS: u8 = 6;
    #[inline]
    fn to_bits(self) -> u64 {
        f64::to_bits(self)
    }
    #[inline]
    fn from_bits(bits: u64) -> Self {
        f64::from_bits(bits)
    }
}

/// Codec state: previous value and the leading / trailing zero window of the last `11` code.
pub(crate) struct XorCodec<T> {
    prev: Option<u64>,
    /// `(leading zeros, trailing zeros)` of the last explicitly coded XOR, if any.
    window: Option<(u8, u8)>,
    _ty: core::marker::PhantomData<T>,
}

impl<T> Clone for XorCodec<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for XorCodec<T> {}

impl<T> Default for XorCodec<T> {
    fn default() -> Self {
        XorCodec {
            prev: None,
            window: None,
            _ty: core::marker::PhantomData,
        }
    }
}

impl<T: XorBits> Codec<T> for XorCodec<T> {
    fn encode(&mut self, value: T, sink: &mut impl BitSink) -> Result<(), Error> {
        let bits = value.to_bits();
        let Some(prev) = self.prev else {
            self.prev = Some(bits);
            return sink.put_bits(T::BITS, bits);
        };
        self.prev = Some(bits);
        let x = bits ^ prev;
        if x == 0 {
            return sink.put_bit(false);
        }
        sink.put_bit(true)?;
        // x is at most BITS wide, so leading zeros counted at 64 bits are offset by the unused high bits
        let lead = (x.leading_zeros() - (64 - T::BITS as u32)) as u8;
        let trail = x.trailing_zeros() as u8;
        if let Some((plead, ptrail)) = self.window
            && lead >= plead
            && trail >= ptrail
        {
            sink.put_bit(false)?;
            return sink.put_bits(T::BITS - plead - ptrail, x >> ptrail);
        }
        let len = T::BITS - lead - trail;
        sink.put_bit(true)?;
        sink.put_bits(T::LEAD_BITS, lead as u64)?;
        sink.put_bits(T::LEAD_BITS, (len - 1) as u64)?;
        sink.put_bits(len, x >> trail)?;
        self.window = Some((lead, trail));
        Ok(())
    }

    fn decode(&mut self, rd: &mut BufReader<'_>) -> Result<T, Error> {
        let Some(prev) = self.prev else {
            let bits = rd.read_un64(T::BITS)?;
            self.prev = Some(bits);
            return Ok(T::from_bits(bits));
        };
        if !rd.read_bool()? {
            return Ok(T::from_bits(prev));
        }
        let x = if !rd.read_bool()? {
            let Some((plead, ptrail)) = self.window else {
                return Err(Error::MalformedSeries); // a window reuse before any window
            };
            rd.read_un64(T::BITS - plead - ptrail)? << ptrail
        } else {
            let lead = rd.read_un64(T::LEAD_BITS)? as u8;
            let len = rd.read_un64(T::LEAD_BITS)? as u8 + 1;
            if lead + len > T::BITS {
                return Err(Error::MalformedSeries);
            }
            let trail = T::BITS - lead - len;
            self.window = Some((lead, trail));
            rd.read_un64(len)? << trail
        };
        let bits = prev ^ x;
        self.prev = Some(bits);
        Ok(T::from_bits(bits))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BufWriter;
    use crate::series::BitCounter;

    /// Encodes `values` with one codec, decodes them back bit for bit, returns the bits written.
    fn round_trip<T: XorBits + core::fmt::Debug>(values: &[T]) -> usize {
        let mut counter = BitCounter::new();
        let mut codec = XorCodec::<T>::default();
        for &v in values {
            codec.encode(v, &mut counter).unwrap();
        }
        let mut buf = [0u8; 4096];
        let mut wr = BufWriter::new(&mut buf);
        let mut codec = XorCodec::<T>::default();
        for &v in values {
            codec.encode(v, &mut wr).unwrap();
        }
        let bytes = wr.finish().unwrap();
        let mut rd = BufReader::new(bytes);
        let mut codec = XorCodec::<T>::default();
        for &v in values {
            assert_eq!(
                codec.decode(&mut rd).unwrap().to_bits(),
                v.to_bits(),
                "{values:?}"
            );
        }
        counter.bits()
    }

    #[test]
    fn special_values() {
        round_trip::<f32>(&[
            0.0,
            -0.0,
            f32::NAN,
            f32::from_bits(0x7FC0_1234), // NaN payload
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::MIN_POSITIVE,
            f32::MAX,
            f32::MIN,
            1e-40, // subnormal
        ]);
        round_trip::<f64>(&[
            0.0,
            -0.0,
            f64::NAN,
            f64::from_bits(0x7FF8_0000_0000_BEEF),
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MIN_POSITIVE,
            f64::MAX,
            f64::MIN,
            5e-324,
        ]);
    }

    #[test]
    fn repeated_values_cost_one_bit() {
        assert_eq!(round_trip::<f32>(&[1.5]), 32);
        assert_eq!(round_trip::<f64>(&[1.5]), 64);
        assert_eq!(round_trip(&[2.5f32; 32]), 32 + 31);
    }

    #[test]
    fn all_bits_change() {
        // every value flips all 32 bits: 11 + 5 + 5 + 32 = 44 bits, then window reuse: 10 + 32 bits
        let v = [
            0.0f32,
            f32::from_bits(u32::MAX),
            0.0,
            f32::from_bits(u32::MAX),
        ];
        assert_eq!(round_trip(&v), 32 + 44 + 34 + 34);
    }

    #[test]
    fn window_reuse() {
        // 1.0 = 0x3F800000, 1.5 = 0x3FC00000: xor 0x00400000 (lead 9, trail 22, len 1)
        // then 1.25 = 0x3FA00000: xor with 1.5 = 0x00600000 (lead 9, trail 21): trail < 22, new window
        // then 1.0: xor 0x3FA00000 ^ 0x3F800000 = 0x00200000 (lead 10, trail 21): fits the window -> 10 + 2 bits
        assert_eq!(
            round_trip(&[1.0f32, 1.5, 1.25, 1.0]),
            32 + (2 + 5 + 5 + 1) + (2 + 5 + 5 + 2) + (2 + 2)
        );
    }

    #[test]
    fn malformed_window_reuse_is_an_error() {
        // first value (32 bits of 0), then a `10` code with no window yet
        let bytes = [0u8, 0, 0, 0, 0b1000_0000];
        let mut rd = BufReader::new(&bytes);
        let mut codec = XorCodec::<f32>::default();
        codec.decode(&mut rd).unwrap();
        assert_eq!(codec.decode(&mut rd), Err(Error::MalformedSeries));
    }
}

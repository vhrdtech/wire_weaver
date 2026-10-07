//! [XorFloat]: `f32` / `f64` sequences as XOR against the previous value (Gorilla).

use super::{BitSink, Codec, sequence_types};
use crate::{BufReader, BufWriter, Error};

/// Floats [XorFloat] takes: `f32` and `f64`, handled as their bit patterns, so NaN payloads, `-0.0` and
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
pub struct XorCodec<T> {
    prev: Option<u64>,
    /// `(leading zeros, trailing zeros)` of the last explicitly coded XOR, if any.
    window: Option<(u8, u8)>,
    _ty: core::marker::PhantomData<T>,
}

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

sequence_types! {
    /// `f32` / `f64` sequence stored as the XOR of each value with the previous one (SW-32), the Gorilla float
    /// code: a repeated value costs `1` bit; a value whose changed bits fall inside the window of the previous
    /// change costs `2` bits plus those bits; otherwise `2` bits, the leading zero count (5 bits for `f32`, 6 for
    /// `f64`), the number of changed bits minus one (same width) and the changed bits. Values are compared bit by
    /// bit, so NaN payloads, `-0.0` and infinities round-trip exactly; a sequence of slowly changing readings, of
    /// values with few significant digits or of integers stored as floats compresses well, full-precision noise
    /// does not (consider a fixed-point [Delta](super::Delta) then).
    ///
    /// ```
    /// use shrink_wrap::prelude::*;
    ///
    /// let temps = [21.5f32, 21.5, 21.5, 21.75, 21.75, 22.0];
    /// let mut buf = [0u8; 32];
    /// let bytes = XorFloat::new(&temps).to_ww_bytes(&mut buf).unwrap();
    /// assert_eq!(bytes.len(), 9); // 32 + 1 + 1 + 15 (11, lead 7, len 3, bits) + 1 + 14 = 64 bits + the count
    /// let back = XorFloat::<f32>::from_ww_bytes(bytes).unwrap();
    /// assert_eq!(back.iter().collect::<Vec<_>>(), temps);
    /// ```
    XorFloat, XorFloatIter, XorFloatOwned, XorBits, XorCodec<T>
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeserializeShrinkWrap, SerializeShrinkWrap};

    fn round_trip<T: XorBits + core::fmt::Debug>(values: &[T]) -> usize {
        let mut buf = [0u8; 4096];
        let bytes = XorFloat::new(values)
            .to_ww_bytes(&mut buf)
            .unwrap()
            .to_vec();
        let back: XorFloat<T> = XorFloat::from_ww_bytes(&bytes).unwrap();
        assert_eq!(back.len(), values.len());
        assert!(
            back.iter()
                .map(T::to_bits)
                .eq(values.iter().map(|v| v.to_bits())),
            "{values:?} != {back:?}"
        );
        #[cfg(feature = "std")]
        {
            use crate::{DeserializeShrinkWrapOwned, SerializeShrinkWrapOwned};
            let owned = XorFloatOwned(values.to_vec());
            assert_eq!(owned.to_ww_bytes_owned().unwrap(), bytes);
            let back = XorFloatOwned::<T>::from_ww_bytes_owned(&bytes).unwrap();
            assert!(
                back.iter()
                    .map(|v| v.to_bits())
                    .eq(values.iter().map(|v| v.to_bits()))
            );
        }
        bytes.len()
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
    fn empty_single_repeated() {
        assert_eq!(round_trip::<f32>(&[]), 1);
        assert_eq!(round_trip::<f32>(&[1.5]), 5);
        assert_eq!(round_trip::<f64>(&[1.5]), 9);
        // 32 + 31 x 1 bit = 63 bits -> 8 bytes, count 32 = two nibbles -> 9
        assert_eq!(round_trip(&[2.5f32; 32]), 9);
    }

    #[test]
    fn all_bits_change() {
        // every value flips all 32 bits: 11 + 5 + 5 + 32 = 44 bits each after the first
        let v = [
            0.0f32,
            f32::from_bits(u32::MAX),
            0.0,
            f32::from_bits(u32::MAX),
        ];
        // 32 + 44 + 2 + 32 (window reuse: 10 + 32 bits) + 2 + 32 = 144 bits = 18 bytes + count nibble
        assert_eq!(round_trip(&v), 19);
    }

    #[test]
    fn window_reuse() {
        // 1.0 = 0x3F800000, 1.5 = 0x3FC00000: xor 0x00400000 (lead 9, trail 22, len 1)
        // then 1.25 = 0x3FA00000: xor with 1.5 = 0x00600000 (lead 9, trail 21): trail < 22, new window
        // then 1.0: xor 0x3FA00000 ^ 0x3F800000 = 0x00200000 (lead 10, trail 21): fits the window -> 10 + 2 bits
        let v = [1.0f32, 1.5, 1.25, 1.0];
        let mut buf = [0u8; 16];
        let bytes = XorFloat::new(&v).to_ww_bytes(&mut buf).unwrap();
        // 32 + (2+5+5+1) + (2+5+5+2) + (2+2) = 63 bits -> 8 bytes + count nibble
        assert_eq!(bytes.len(), 9);
        // the first value goes through the bit cursor, most significant bit first
        assert_eq!(bytes[0..4], 1.0f32.to_bits().to_be_bytes());
    }

    #[test]
    fn malformed_window_reuse_is_an_error() {
        // first value (32 bits of 0), then a `10` code with no window yet
        let bytes = [0u8, 0, 0, 0, 0b1000_0000, 0x02];
        assert_eq!(
            XorFloat::<f32>::from_ww_bytes(&bytes),
            Err(Error::MalformedSeries)
        );
    }

    #[test]
    fn out_of_bits_is_an_error() {
        let mut buf = [0u8; 8];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_rev_len(2).unwrap();
        wr.write_u8(1).unwrap();
        wr.write_u8(2).unwrap();
        let bytes = wr.finish().unwrap();
        assert!(XorFloat::<f32>::from_ww_bytes(bytes).is_err());
        assert_eq!(
            XorFloat::<f32>::from_ww_bytes(&[]),
            Err(Error::OutOfBoundsRev)
        );
        // count 10 in 2 data bytes
        let mut wr = BufWriter::new(&mut buf);
        wr.write_rev_len(10).unwrap();
        wr.write_u8(1).unwrap();
        let bytes = wr.finish().unwrap();
        assert!(XorFloat::<f64>::from_ww_bytes(bytes).is_err());
    }
}

//! [Delta] and [DeltaOfDelta]: integer sequences as differences.

use super::{
    BitSink, Codec, get_delta, mask, put_delta, sequence_types, sign_extend, unzigzag, zigzag,
};
use crate::{BufReader, BufWriter, Error};

/// Integers [Delta] and [DeltaOfDelta] take: `u8`..`u64` and `i8`..`i64`. Values are handled as their two's
/// complement bit pattern at the type's width, so wrapping differences are always representable and never fail.
pub trait DeltaInt: Copy + sealed::Sealed {
    /// Bits on the wire for the first element and the widest delta bucket.
    const BITS: u8;
    /// Zero-extended bit pattern.
    fn to_bits(self) -> u64;
    /// From the low [BITS](Self::BITS) bits, sign-extended for signed types.
    fn from_bits(bits: u64) -> Self;
}

mod sealed {
    pub trait Sealed {}
}

macro_rules! delta_int {
    ($($ty:ty),*) => {$(
        impl sealed::Sealed for $ty {}
        impl DeltaInt for $ty {
            const BITS: u8 = <$ty>::BITS as u8;
            #[inline]
            fn to_bits(self) -> u64 {
                // `as u64` of a negative signed value sign-extends; keep the type's own bits only
                (self as i64 as u64) & mask(<Self as DeltaInt>::BITS)
            }
            #[inline]
            fn from_bits(bits: u64) -> Self {
                sign_extend(bits & mask(<Self as DeltaInt>::BITS), <Self as DeltaInt>::BITS) as $ty
            }
        }
    )*};
}

delta_int!(u8, u16, u32, u64, i8, i16, i32, i64);

/// Codec state: `DOD` false encodes each value as the difference to the previous one, true as the difference of
/// that difference to the previous difference (Gorilla timestamps).
pub struct DeltaCodec<T, const DOD: bool> {
    prev: Option<u64>,
    prev_delta: i64,
    _ty: core::marker::PhantomData<T>,
}

impl<T, const DOD: bool> Default for DeltaCodec<T, DOD> {
    fn default() -> Self {
        DeltaCodec {
            prev: None,
            prev_delta: 0,
            _ty: core::marker::PhantomData,
        }
    }
}

impl<T: DeltaInt, const DOD: bool> Codec<T> for DeltaCodec<T, DOD> {
    fn encode(&mut self, value: T, sink: &mut impl BitSink) -> Result<(), Error> {
        let bits = value.to_bits();
        let Some(prev) = self.prev else {
            self.prev = Some(bits);
            return sink.put_bits(T::BITS, bits);
        };
        let delta = sign_extend(bits.wrapping_sub(prev) & mask(T::BITS), T::BITS);
        let coded = if DOD {
            // deltas are W-bit signed, their difference fits W bits again when wrapped at W
            sign_extend(
                (delta.wrapping_sub(self.prev_delta) as u64) & mask(T::BITS),
                T::BITS,
            )
        } else {
            delta
        };
        self.prev = Some(bits);
        self.prev_delta = delta;
        put_delta(sink, zigzag(coded) & mask(T::BITS), T::BITS)
    }

    fn decode(&mut self, rd: &mut BufReader<'_>) -> Result<T, Error> {
        let Some(prev) = self.prev else {
            let bits = rd.read_un64(T::BITS)?;
            self.prev = Some(bits);
            return Ok(T::from_bits(bits));
        };
        let coded = unzigzag(get_delta(rd, T::BITS)?);
        let delta = if DOD {
            sign_extend(
                (coded.wrapping_add(self.prev_delta) as u64) & mask(T::BITS),
                T::BITS,
            )
        } else {
            coded
        };
        let bits = prev.wrapping_add(delta as u64) & mask(T::BITS);
        self.prev = Some(bits);
        self.prev_delta = delta;
        Ok(T::from_bits(bits))
    }
}

sequence_types! {
    /// Integer sequence stored as differences between consecutive values (SW-31): counters, slowly changing
    /// readings, fixed-point measurements. The first value takes the full width of `T`, every next one `1` bit
    /// when equal to the previous, `9` bits for a difference within `-64..=63`, `12` within `-256..=255`, `16`
    /// within `-2048..=2047`, then `25` and `38` bits, and the full width plus a prefix for anything else. Use
    /// [DeltaOfDelta] for values that grow by a near-constant step (timestamps).
    ///
    /// ```
    /// use shrink_wrap::prelude::*;
    ///
    /// #[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
    /// struct Readings<'i> {
    ///     id: u8,
    ///     temps: Delta<'i, i16>, // tenths of a degree
    /// }
    ///
    /// let mut buf = [0u8; 32];
    /// let r = Readings { id: 7, temps: Delta::new(&[215, 215, 216, 216, 214]) };
    /// let bytes = r.to_ww_bytes(&mut buf).unwrap();
    /// assert_eq!(bytes.len(), 6); // 1 + 2 + (1 + 9 + 1 + 9 bits -> 3) with the count in the last nibble
    /// let back = Readings::from_ww_bytes(bytes).unwrap();
    /// assert_eq!(back.temps.iter().collect::<Vec<_>>(), [215, 215, 216, 216, 214]);
    /// ```
    Delta, DeltaIter, DeltaOwned, DeltaInt, DeltaCodec<T, false>
}

sequence_types! {
    /// Integer sequence stored as the difference between consecutive differences (SW-31), the Gorilla timestamp
    /// code: a value that grows by the same step as the one before costs `1` bit, so regular timestamps and
    /// steady counters compress to almost nothing. Same codes as [Delta], applied to the change of the difference.
    ///
    /// ```
    /// use shrink_wrap::prelude::*;
    ///
    /// let ts: [u32; 6] = [1_700_000_000, 1_700_000_010, 1_700_000_020, 1_700_000_030, 1_700_000_041, 1_700_000_050];
    /// let mut buf = [0u8; 32];
    /// let bytes = DeltaOfDelta::new(&ts).to_ww_bytes(&mut buf).unwrap();
    /// assert_eq!(bytes.len(), 9); // 32 bits, 9 (step 10), 1, 1, 9 (+1), 9 (-1) = 61 bits, then the count nibble
    /// assert_eq!(DeltaOfDelta::<u32>::from_ww_bytes(bytes).unwrap().iter().collect::<Vec<_>>(), ts);
    /// ```
    DeltaOfDelta, DeltaOfDeltaIter, DeltaOfDeltaOwned, DeltaInt, DeltaCodec<T, true>
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeserializeShrinkWrap, SerializeShrinkWrap};

    fn round_trip<T: DeltaInt + PartialEq + core::fmt::Debug>(values: &[T]) -> usize {
        let mut buf = [0u8; 4096];
        let bytes = Delta::new(values).to_ww_bytes(&mut buf).unwrap().to_vec();
        let back: Delta<T> = Delta::from_ww_bytes(&bytes).unwrap();
        assert_eq!(back.len(), values.len());
        assert!(back.iter().eq(values.iter().copied()), "{values:?}");
        let dd_bytes = DeltaOfDelta::new(values)
            .to_ww_bytes(&mut buf)
            .unwrap()
            .to_vec();
        let back: DeltaOfDelta<T> = DeltaOfDelta::from_ww_bytes(&dd_bytes).unwrap();
        assert!(back.iter().eq(values.iter().copied()), "dod {values:?}");
        #[cfg(feature = "std")]
        {
            use crate::{DeserializeShrinkWrapOwned, SerializeShrinkWrapOwned};
            let owned = DeltaOwned(values.to_vec());
            assert_eq!(owned.to_ww_bytes_owned().unwrap(), bytes);
            assert_eq!(DeltaOwned::<T>::from_ww_bytes_owned(&bytes).unwrap(), owned);
            let owned = DeltaOfDeltaOwned(values.to_vec());
            assert_eq!(owned.to_ww_bytes_owned().unwrap(), dd_bytes);
            assert_eq!(
                DeltaOfDeltaOwned::<T>::from_ww_bytes_owned(&dd_bytes).unwrap(),
                owned
            );
        }
        bytes.len()
    }

    #[test]
    fn extremes_every_type() {
        round_trip::<u8>(&[0, 255, 0, 128, 127, 1]);
        round_trip::<i8>(&[-128, 127, -128, 0, -1, 1]);
        round_trip::<u16>(&[0, u16::MAX, 1, 0x8000]);
        round_trip::<i16>(&[i16::MIN, i16::MAX, 0, -1]);
        round_trip::<u32>(&[0, u32::MAX, 1, 0x8000_0000, 0x7FFF_FFFF]);
        round_trip::<i32>(&[i32::MIN, i32::MAX, 0, -1, 1]);
        round_trip::<u64>(&[0, u64::MAX, 1, 1 << 63, (1 << 63) - 1, 12345]);
        round_trip::<i64>(&[i64::MIN, i64::MAX, 0, -1, 1, i64::MIN + 1]);
    }

    #[test]
    fn empty_and_single() {
        assert_eq!(round_trip::<u32>(&[]), 1); // the zero count nibble
        assert_eq!(round_trip::<u32>(&[7]), 5); // 32 bits + count nibble
        assert_eq!(round_trip::<u8>(&[7]), 2);
        assert_eq!(round_trip::<u64>(&[7]), 9);
    }

    #[test]
    fn repeated_values_cost_one_bit() {
        let v = [42u32; 63];
        // 32 bits + 62 x 1 bit = 94 bits -> 12 bytes, count 63 = two nibbles -> 13
        assert_eq!(round_trip(&v), 13);
    }

    #[test]
    fn bytes_delta_u8() {
        let mut buf = [0u8; 16];
        let bytes = Delta::new(&[10u8, 10, 11, 9, 200])
            .to_ww_bytes(&mut buf)
            .unwrap();
        // 00001010 | 0 | 10 0000010 (zigzag(1)=2) | 10 0000011 (zigzag(-2)=3) | 11 10111110 (zigzag(191)=382 -> u8 wraps: 191 as 8-bit signed = -65, zigzag = 129 -> 11 10000001)
        // first byte, then bits: 0 100000010 100000011 1110000001 = 29 bits -> 4 bytes, plus the count nibble
        assert_eq!(bytes.len(), 1 + 4 + 1);
        let back: Delta<u8> = Delta::from_ww_bytes(bytes).unwrap();
        assert_eq!(back.iter().collect::<Vec<_>>(), [10, 10, 11, 9, 200]);
    }

    #[test]
    fn out_of_bits_is_an_error() {
        // count 3, but only 2 bytes of data: the first u32 alone needs 4
        let mut buf = [0u8; 8];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_rev_len(3).unwrap();
        wr.write_u8(1).unwrap();
        wr.write_u8(2).unwrap();
        let bytes = wr.finish().unwrap();
        assert_eq!(
            Delta::<u32>::from_ww_bytes(bytes),
            Err(Error::OutOfBoundsReadUN)
        );
    }

    #[test]
    fn count_beyond_data_is_malformed() {
        // count 200 in the reverse length, 1 byte of data: every element takes at least a bit
        let mut buf = [0u8; 8];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_rev_len(200).unwrap();
        wr.write_u8(0).unwrap();
        let bytes = wr.finish().unwrap();
        assert_eq!(
            Delta::<u8>::from_ww_bytes(bytes),
            Err(Error::MalformedSeries)
        );
        assert_eq!(
            DeltaOfDelta::<u8>::from_ww_bytes(bytes),
            Err(Error::MalformedSeries)
        );
        #[cfg(feature = "std")]
        assert_eq!(
            <DeltaOwned<u8> as crate::DeserializeShrinkWrapOwned>::from_ww_bytes_owned(bytes),
            Err(Error::MalformedSeries)
        );
    }

    #[test]
    fn delta_of_delta_regular_step_is_one_bit() {
        let ts: Vec<u64> = (0..100).map(|i| 1_700_000_000 + i * 60).collect();
        let mut buf = [0u8; 256];
        let bytes = DeltaOfDelta::new(&ts).to_ww_bytes(&mut buf).unwrap();
        // 64 + 9 (first delta 60) + 98 x 1 = 171 bits = 22 bytes + 2 count nibbles
        assert_eq!(bytes.len(), 23);
    }
}

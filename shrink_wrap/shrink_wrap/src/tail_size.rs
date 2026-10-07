use core::fmt::{Debug, Display, Formatter};

use crate::vlq32::UVlq32;
use crate::{
    BufReader, BufWriter, DeserializeShrinkWrap, DeserializeShrinkWrapOwned, ElementSize, Error,
    SerializeShrinkWrap,
};

/// Size of the rest of the enclosing value, in bytes: filled in by `#[derive_shrink_wrap(..)]` once the value is
/// serialized, and used by the reader to bound the value, or to skip it without parsing.
///
/// On the wire it is a [UVlq32] right-justified in `N` bytes (1 to 5, 5 by default), padded with empty `0x80`
/// groups in front, so any `UVlq32` reader decodes it. The value counts the bytes from the end of the slot to the
/// end of the value it is in, including the reverse lengths that value's own fields put at the back. `N` is the
/// capacity of the slot: the value must fit into `7 * N` bits (127 bytes for `N = 1`, 16 KB for 2, 2 MB for 3,
/// 256 MB for 4, 4 GB for 5), serialization fails with [Error::LenTooLong] otherwise.
///
/// In a struct or enum variant, the generated code ignores the field's value when writing (put `TailSize(0)` or
/// `Default::default()`), fills the slot in once the fields after it are written, and when reading, reads the slot
/// and bounds the rest of the value to it: a buffer longer than the value (a file region, a record at a fixed
/// offset) stops at the right byte, a truncated one fails with [Error::OutOfBoundsSplit] instead of reading
/// garbage, and bytes a newer writer appended after the fields this reader knows are skipped. Rules the macro
/// checks: one `TailSize` per struct or variant, a plain field (not inside `Option`, `Vec`, tuples or arrays), only
/// in `Unsized` types (no `sized`, `final_structure` or `self_describing`), and every field before it must be
/// `Sized` or `SelfDescribing`, because a `Vec`, `String` or `Unsized` field keeps its length at the back of the
/// buffer, which is not where the value ends when the buffer is longer than it. The slot, its position and `N` are
/// part of the wire layout: add it in the first version of a type, like any other field that is not evolvable.
///
/// Standalone (outside the derive macro), `TailSize(n)` serializes `n` padded to `N` bytes and reads back the
/// same way, like [UVlq32Backfill](crate::UVlq32Backfill) with a chosen width. To write a value by hand, use
/// [BufWriter::reserve_tail_size] and [BufWriter::backfill_tail_size] (same on `BufWriterOwned`).
///
/// ```
/// use shrink_wrap::prelude::*;
///
/// #[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
/// struct Header<'i> {
///     magic: u16,
///     size: TailSize<2>,
///     name: &'i str,
/// }
///
/// let mut region = [0xFFu8; 16]; // longer than the value, like a file region
/// let header = Header { magic: 0xABCD, size: TailSize(0), name: "ab" };
/// let len = header.to_ww_bytes(&mut region).unwrap().len();
/// assert_eq!(&region[..len], &[0xCD, 0xAB, 0x80, 0x03, b'a', b'b', 0x02]);
/// let back = Header::from_ww_bytes(&region).unwrap(); // the whole region, bounded by the slot
/// assert_eq!(back, Header { magic: 0xABCD, size: TailSize(3), name: "ab" });
/// ```
#[derive(Copy, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct TailSize<const N: usize = 5>(pub u32);

/// Longest `UVlq32` and so the largest slot width.
const MAX_LEN: usize = 5;

impl<const N: usize> TailSize<N> {
    /// Bytes the slot takes on the wire.
    pub const LEN: usize = {
        assert!(N >= 1 && N <= 5, "TailSize<N>: N must be 1 to 5 bytes");
        N
    };

    /// The slot bytes for `value`: a `UVlq32` right-justified in `N` bytes, or [Error::LenTooLong] if it does not
    /// fit.
    pub(crate) fn encode(value: u32) -> Result<[u8; MAX_LEN], Error> {
        encode_slot(value, Self::LEN)
    }

    /// Decode the slot bytes, which must hold a `UVlq32` spanning exactly `N` bytes.
    pub(crate) fn decode(slot: &[u8]) -> Result<Self, Error> {
        Ok(TailSize(decode_slot(slot)?))
    }
}

/// Right-justified in the last `len` bytes of the returned array.
pub(crate) fn encode_slot(value: u32, len: usize) -> Result<[u8; MAX_LEN], Error> {
    if UVlq32(value).len_bytes() > len {
        return Err(Error::LenTooLong);
    }
    Ok(UVlq32(value).encode_right_justified().0)
}

pub(crate) fn decode_slot(slot: &[u8]) -> Result<u32, Error> {
    let mut rd = BufReader::new(slot);
    let value = UVlq32::read_forward(&mut rd)?;
    if rd.bytes_left() != 0 {
        // a slot is always written in full, a shorter number means the bytes are not a slot
        return Err(Error::MalformedUVlq32);
    }
    Ok(value.0)
}

impl<const N: usize> SerializeShrinkWrap for TailSize<N> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: N * 8 };

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        let bytes = Self::encode(self.0)?;
        wr.write_raw_slice(&bytes[MAX_LEN - Self::LEN..])
    }
}

#[cfg(feature = "std")]
impl<const N: usize> crate::SerializeShrinkWrapOwned for TailSize<N> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: N * 8 };

    fn ser_shrink_wrap_owned(&self, wr: &mut crate::BufWriterOwned) -> Result<(), Error> {
        let bytes = Self::encode(self.0)?;
        wr.write_raw_slice(&bytes[MAX_LEN - Self::LEN..])
    }
}

impl<'i, const N: usize> DeserializeShrinkWrap<'i> for TailSize<N> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: N * 8 };

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        Self::decode(rd.read_raw_slice(Self::LEN)?)
    }
}

impl<const N: usize> DeserializeShrinkWrapOwned for TailSize<N> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Sized { size_bits: N * 8 };

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, Error> {
        Self::decode(rd.read_raw_slice(Self::LEN)?)
    }
}

impl<const N: usize> From<TailSize<N>> for u32 {
    fn from(value: TailSize<N>) -> Self {
        value.0
    }
}

impl<const N: usize> From<u32> for TailSize<N> {
    fn from(num: u32) -> Self {
        TailSize(num)
    }
}

impl<const N: usize> Debug for TailSize<N> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<const N: usize> Display for TailSize<N> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BufReader, BufWriter, DeserializeShrinkWrap, Error, SerializeShrinkWrap};

    #[test]
    fn padded_widths() {
        let mut buf = [0u8; 8];
        assert_eq!(TailSize::<1>(0).to_ww_bytes(&mut buf).unwrap(), &[0x00]);
        assert_eq!(TailSize::<1>(127).to_ww_bytes(&mut buf).unwrap(), &[0x7f]);
        assert_eq!(
            TailSize::<2>(300).to_ww_bytes(&mut buf).unwrap(),
            &[0x82, 0x2c]
        );
        assert_eq!(
            TailSize::<3>(300).to_ww_bytes(&mut buf).unwrap(),
            &[0x80, 0x82, 0x2c]
        );
        assert_eq!(
            TailSize::<5>(0).to_ww_bytes(&mut buf).unwrap(),
            &[0x80, 0x80, 0x80, 0x80, 0x00]
        );
        assert_eq!(
            TailSize::<5>(u32::MAX).to_ww_bytes(&mut buf).unwrap(),
            &[0x8f, 0xff, 0xff, 0xff, 0x7f]
        );
    }

    #[test]
    fn too_long_for_width() {
        let mut buf = [0u8; 8];
        assert_eq!(
            TailSize::<1>(128).to_ww_bytes(&mut buf),
            Err(Error::LenTooLong)
        );
        assert_eq!(
            TailSize::<2>(16_384).to_ww_bytes(&mut buf),
            Err(Error::LenTooLong)
        );
        assert!(TailSize::<2>(16_383).to_ww_bytes(&mut buf).is_ok());
    }

    #[test]
    fn round_trip_all_widths() {
        for value in [
            0,
            1,
            127,
            128,
            16_383,
            16_384,
            0x1f_ffff,
            0x20_0000,
            u32::MAX,
        ] {
            let mut buf = [0u8; 8];
            let bytes = TailSize::<5>(value).to_ww_bytes(&mut buf).unwrap();
            assert_eq!(bytes.len(), 5);
            assert_eq!(TailSize::<5>::from_ww_bytes(bytes), Ok(TailSize(value)));
            // same number for a plain UVlq32 reader
            assert_eq!(UVlq32::from_ww_bytes(bytes), Ok(UVlq32(value)));
            assert_eq!(
                TailSize::<5>::from_ww_bytes_owned(bytes),
                Ok(TailSize(value))
            );
        }
        let mut buf = [0u8; 8];
        let bytes = TailSize::<2>(300).to_ww_bytes(&mut buf).unwrap();
        assert_eq!(TailSize::<2>::from_ww_bytes(bytes), Ok(TailSize(300)));
    }

    #[test]
    fn decode_is_strict() {
        // a number shorter than the slot is not a slot (e.g. a zeroed file region)
        assert_eq!(
            TailSize::<2>::from_ww_bytes(&[0x00, 0x00]),
            Err(Error::MalformedUVlq32)
        );
        // an unfinished number
        assert_eq!(
            TailSize::<2>::from_ww_bytes(&[0x80, 0x80]),
            Err(Error::OutOfBoundsReadU8)
        );
        // too short a buffer
        assert_eq!(
            TailSize::<2>::from_ww_bytes(&[0x80]),
            Err(Error::OutOfBoundsReadRawSlice)
        );
    }

    #[test]
    fn reserve_and_backfill() {
        let mut buf = [0u8; 32];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_u8(0xAA).unwrap();
        let slot = wr.reserve_tail_size(2).unwrap();
        wr.write_str("abc").unwrap();
        wr.write_bool(true).unwrap();
        wr.backfill_tail_size(slot).unwrap();
        let bytes = wr.finish().unwrap();
        // "abc", then the bool and the string's reversed length nibble share a byte: 4 bytes after the slot
        assert_eq!(bytes, &[0xAA, 0x80, 0x04, b'a', b'b', b'c', 0x83]);
        let mut rd = BufReader::new(bytes);
        assert_eq!(rd.read_u8(), Ok(0xAA));
        let size: TailSize<2> = rd.read().unwrap();
        assert_eq!(size.0, 4);
        let mut rest = rd.split(size.0 as usize).unwrap();
        assert_eq!(rest.read_str(), Ok("abc"));
        assert_eq!(rest.read_bool(), Ok(true));
        assert_eq!(rd.bytes_left(), 0);
    }

    #[test]
    fn backfill_does_not_touch_lengths_before_the_slot() {
        let mut buf = [0u8; 32];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_str("xy").unwrap();
        let slot = wr.reserve_tail_size(1).unwrap();
        wr.write_str("abc").unwrap();
        wr.backfill_tail_size(slot).unwrap();
        let bytes = wr.finish().unwrap();
        // "abc" and its length are counted (4), "xy"'s length is encoded by finish, after the slot's range
        assert_eq!(bytes, &[b'x', b'y', 0x04, b'a', b'b', b'c', 0x03, 0x02]);
    }

    #[test]
    fn backfill_too_long_for_width() {
        let mut buf = [0u8; 160];
        let mut wr = BufWriter::new(&mut buf);
        let slot = wr.reserve_tail_size(1).unwrap();
        wr.write_raw_slice(&[0u8; 128]).unwrap();
        assert_eq!(wr.backfill_tail_size(slot), Err(Error::LenTooLong));
    }

    #[test]
    fn reserve_out_of_bounds() {
        let mut buf = [0u8; 2];
        let mut wr = BufWriter::new(&mut buf);
        assert!(wr.reserve_tail_size(5).is_err());
    }

    #[cfg(feature = "std")]
    #[test]
    fn owned_writer_matches() {
        let mut buf = [0u8; 32];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_str("xy").unwrap();
        wr.write_bool(true).unwrap();
        let slot = wr.reserve_tail_size(3).unwrap();
        wr.write_bool(false).unwrap();
        wr.write_bytes(&[1, 2, 3]).unwrap();
        wr.write_str("abc").unwrap();
        wr.backfill_tail_size(slot).unwrap();
        wr.write_u8(0xEE).unwrap();
        let expected = wr.finish().unwrap().to_vec();

        let mut wro = crate::BufWriterOwned::new();
        wro.write_str("xy").unwrap();
        wro.write_bool(true).unwrap();
        let slot = wro.reserve_tail_size(3).unwrap();
        wro.write_bool(false).unwrap();
        wro.write_bytes(&[1, 2, 3]).unwrap();
        wro.write_str("abc").unwrap();
        wro.backfill_tail_size(slot).unwrap();
        wro.write_u8(0xEE).unwrap();
        assert_eq!(wro.finish().unwrap(), expected);

        let mut rd = BufReader::new(&expected);
        assert_eq!(rd.read_str(), Ok("xy"));
        assert_eq!(rd.read_bool(), Ok(true));
        let size: TailSize<3> = rd.read().unwrap();
        let mut rest = rd.split(size.0 as usize).unwrap();
        assert_eq!(rest.read_bool(), Ok(false));
        assert_eq!(rest.read_bytes(), Ok(&[1u8, 2, 3][..]));
        assert_eq!(rest.read_str(), Ok("abc"));
        assert_eq!(rest.bytes_left(), 0);
        assert_eq!(rd.read_u8(), Ok(0xEE));
    }
}

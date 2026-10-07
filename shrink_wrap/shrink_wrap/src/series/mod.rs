//! Compressed sequences (SW-31, SW-32): [Delta], [DeltaOfDelta] for integers and [XorFloat] for `f32` / `f64`,
//! used as struct fields like `Vec<T>` but written bit by bit so that a sequence of similar values takes a fraction
//! of the space. Borrowed variants decode lazily from the buffer with no allocation (`no_std`), `..Owned` variants
//! hold a `Vec<T>`.
//!
//! All three share the layout of `Vec<T>` around the elements: `UnsizedFinalStructure`, the element count as a
//! reverse length at the back of the enclosing value, the first element at the full width of `T`, every further
//! element as a prefix code against the one before it, with no alignment in between or after (the bit cursor
//! continues; a byte-aligned field written next aligns itself as usual). The codes are fixed once released, like
//! any other part of the wire format; `docs/serdes/shrink_wrap.md` has them bit by bit.

use crate::{BufReader, BufWriter, Error};

mod delta;
mod xor_float;

pub use delta::{Delta, DeltaInt, DeltaIter, DeltaOfDelta, DeltaOfDeltaIter};
#[cfg(feature = "std")]
pub use delta::{DeltaOfDeltaOwned, DeltaOwned};
#[cfg(feature = "std")]
pub use xor_float::XorFloatOwned;
pub use xor_float::{XorBits, XorFloat, XorFloatIter};

/// Bit-level writes shared by [BufWriter] and `BufWriterOwned`, so a codec is written once.
pub(crate) trait BitSink {
    fn put_bit(&mut self, bit: bool) -> Result<(), Error>;
    /// `count` low bits of `value`, 1 to 64.
    fn put_bits(&mut self, count: u8, value: u64) -> Result<(), Error>;
}

impl BitSink for BufWriter<'_> {
    fn put_bit(&mut self, bit: bool) -> Result<(), Error> {
        self.write_bool(bit)
    }

    fn put_bits(&mut self, count: u8, value: u64) -> Result<(), Error> {
        self.write_un64(count, value)
    }
}

#[cfg(feature = "std")]
impl BitSink for crate::BufWriterOwned {
    fn put_bit(&mut self, bit: bool) -> Result<(), Error> {
        self.write_bool(bit)
    }

    fn put_bits(&mut self, count: u8, value: u64) -> Result<(), Error> {
        self.write_un64(count, value)
    }
}

/// One element codec: the same state drives encoding and decoding.
pub(crate) trait Codec<T>: Default {
    fn encode(&mut self, value: T, sink: &mut impl BitSink) -> Result<(), Error>;
    fn decode(&mut self, rd: &mut BufReader<'_>) -> Result<T, Error>;
}

/// `count` elements claimed by a reverse length against what the reader has left: every element takes at least
/// one bit, so more elements than bits is malformed data, and bounds what an owned decoder allocates.
pub(crate) fn check_count(count: usize, rd: &BufReader<'_>) -> Result<(), Error> {
    if count > rd.bits_left() {
        return Err(Error::MalformedSeries);
    }
    Ok(())
}

/// The borrowed sequence, its iterator and the owned sequence of one codec, with the four serdes traits. Mirrors
/// `RefVec<'i, T>` / `Vec<T>`: the borrowed one is either a slice (to serialize) or a bounded reader (after
/// deserializing), decoded again on every iteration; the owned one wraps a `Vec<T>`.
macro_rules! sequence_types {
    (
        $(#[$doc:meta])*
        $name:ident, $iter:ident, $owned:ident, $bound:ident, $codec:ty
    ) => {
        $(#[$doc])*
        #[derive(Clone, Copy)]
        pub enum $name<'i, T> {
            /// Values to serialize.
            Slice(&'i [T]),
            /// Encoded values, positioned at the first element (what deserializing produces).
            Buf { buf: BufReader<'i>, count: usize },
        }

        impl<'i, T> $name<'i, T> {
            pub const fn new(values: &'i [T]) -> Self {
                $name::Slice(values)
            }

            pub fn len(&self) -> usize {
                match self {
                    $name::Slice(s) => s.len(),
                    $name::Buf { count, .. } => *count,
                }
            }

            pub fn is_empty(&self) -> bool {
                self.len() == 0
            }
        }

        impl<'i, T: $bound> $name<'i, T> {
            pub fn iter(&self) -> $iter<'i, T> {
                match self {
                    $name::Slice(s) => $iter::Slice { slice: s, pos: 0 },
                    $name::Buf { buf, count } => $iter::Buf {
                        buf: *buf,
                        count: *count,
                        pos: 0,
                        codec: Default::default(),
                    },
                }
            }

            /// `to_vec()` of the decoded values.
            #[cfg(feature = "std")]
            pub fn to_owned_seq(&self) -> $owned<T> {
                $owned(self.iter().collect())
            }
        }

        impl<T> Default for $name<'_, T> {
            fn default() -> Self {
                $name::Slice(&[])
            }
        }

        impl<'i, T: $bound> IntoIterator for &$name<'i, T> {
            type Item = T;
            type IntoIter = $iter<'i, T>;

            fn into_iter(self) -> Self::IntoIter {
                self.iter()
            }
        }

        impl<T: $bound + core::fmt::Debug> core::fmt::Debug for $name<'_, T> {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.debug_list().entries(self.iter()).finish()
            }
        }

        #[cfg(feature = "defmt")]
        impl<T: $bound> defmt::Format for $name<'_, T> {
            fn format(&self, fmt: defmt::Formatter) {
                defmt::write!(fmt, "{}({} elements)", stringify!($name), self.len())
            }
        }

        impl<T: $bound + PartialEq> PartialEq for $name<'_, T> {
            fn eq(&self, other: &Self) -> bool {
                self.len() == other.len() && self.iter().eq(other.iter())
            }
        }

        /// Iterator of [`
        #[doc = stringify!($name)]
        /// `], decoding as it goes; stops early on malformed data, which deserialization already rejected.
        pub enum $iter<'i, T> {
            Slice { slice: &'i [T], pos: usize },
            Buf { buf: BufReader<'i>, count: usize, pos: usize, codec: $codec },
        }

        impl<T: $bound> Iterator for $iter<'_, T> {
            type Item = T;

            fn next(&mut self) -> Option<T> {
                match self {
                    $iter::Slice { slice, pos } => {
                        let v = *slice.get(*pos)?;
                        *pos += 1;
                        Some(v)
                    }
                    $iter::Buf { buf, count, pos, codec } => {
                        if *pos >= *count {
                            return None;
                        }
                        *pos += 1;
                        match codec.decode(buf) {
                            Ok(v) => Some(v),
                            Err(_) => {
                                *pos = *count;
                                None
                            }
                        }
                    }
                }
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                let left = match self {
                    $iter::Slice { slice, pos } => slice.len() - *pos,
                    $iter::Buf { count, pos, .. } => *count - *pos,
                };
                (0, Some(left))
            }
        }

        impl<'i, T: $bound> crate::SerializeShrinkWrap for $name<'i, T> {
            const ELEMENT_SIZE: crate::ElementSize = crate::ElementSize::UnsizedFinalStructure;

            fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
                wr.write_rev_len(self.len())?;
                let mut codec = <$codec>::default();
                for v in self.iter() {
                    codec.encode(v, wr)?;
                }
                Ok(())
            }
        }

        #[cfg(feature = "std")]
        impl<'i, T: $bound> crate::SerializeShrinkWrapOwned for $name<'i, T> {
            const ELEMENT_SIZE: crate::ElementSize = crate::ElementSize::UnsizedFinalStructure;

            fn ser_shrink_wrap_owned(&self, wr: &mut crate::BufWriterOwned) -> Result<(), Error> {
                wr.write_rev_len(self.len())?;
                let mut codec = <$codec>::default();
                for v in self.iter() {
                    codec.encode(v, wr)?;
                }
                Ok(())
            }
        }

        impl<'i, T: $bound> crate::DeserializeShrinkWrap<'i> for $name<'i, T> {
            const ELEMENT_SIZE: crate::ElementSize = crate::ElementSize::UnsizedFinalStructure;

            fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
                let count = rd.read_rev_len()?;
                $crate::series::check_count(count, rd)?;
                let buf = *rd;
                // decode once to validate and to advance the reader past the elements
                let mut codec = <$codec>::default();
                for _ in 0..count {
                    codec.decode(rd)?;
                }
                Ok($name::Buf { buf, count })
            }
        }

        /// Owned [`
        #[doc = stringify!($name)]
        /// `]: a `Vec<T>` encoded the same way.
        #[cfg(feature = "std")]
        #[derive(Clone, Debug, Default, PartialEq)]
        pub struct $owned<T>(pub Vec<T>);

        #[cfg(feature = "std")]
        impl<T> $owned<T> {
            pub const fn new() -> Self {
                $owned(Vec::new())
            }

            pub fn into_inner(self) -> Vec<T> {
                self.0
            }
        }

        #[cfg(feature = "std")]
        impl<T> From<Vec<T>> for $owned<T> {
            fn from(values: Vec<T>) -> Self {
                $owned(values)
            }
        }

        #[cfg(feature = "std")]
        impl<T> FromIterator<T> for $owned<T> {
            fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
                $owned(iter.into_iter().collect())
            }
        }

        #[cfg(feature = "std")]
        impl<T> core::ops::Deref for $owned<T> {
            type Target = Vec<T>;

            fn deref(&self) -> &Vec<T> {
                &self.0
            }
        }

        #[cfg(feature = "std")]
        impl<T> core::ops::DerefMut for $owned<T> {
            fn deref_mut(&mut self) -> &mut Vec<T> {
                &mut self.0
            }
        }

        #[cfg(feature = "std")]
        impl<T: $bound> $owned<T> {
            /// Borrow as the zero-copy type, e.g. to serialize into a `BufWriter`.
            pub fn as_borrowed(&self) -> $name<'_, T> {
                $name::Slice(&self.0)
            }
        }

        #[cfg(feature = "std")]
        impl<T: $bound> crate::SerializeShrinkWrap for $owned<T> {
            const ELEMENT_SIZE: crate::ElementSize = crate::ElementSize::UnsizedFinalStructure;

            fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
                self.as_borrowed().ser_shrink_wrap(wr)
            }
        }

        #[cfg(feature = "std")]
        impl<T: $bound> crate::SerializeShrinkWrapOwned for $owned<T> {
            const ELEMENT_SIZE: crate::ElementSize = crate::ElementSize::UnsizedFinalStructure;

            fn ser_shrink_wrap_owned(&self, wr: &mut crate::BufWriterOwned) -> Result<(), Error> {
                self.as_borrowed().ser_shrink_wrap_owned(wr)
            }
        }

        #[cfg(feature = "std")]
        impl<T: $bound> crate::DeserializeShrinkWrapOwned for $owned<T> {
            const ELEMENT_SIZE: crate::ElementSize = crate::ElementSize::UnsizedFinalStructure;

            fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, Error> {
                let count = rd.read_rev_len()?;
                $crate::series::check_count(count, rd)?;
                // pushed one by one: the count is data, not a reason to allocate up front
                let mut values = Vec::with_capacity(count.min(1024));
                let mut codec = <$codec>::default();
                for _ in 0..count {
                    values.push(codec.decode(rd)?);
                }
                Ok($owned(values))
            }
        }
    };
}

pub(crate) use sequence_types;

/// Zigzag: small negative and positive numbers become small unsigned ones.
#[inline]
pub(crate) const fn zigzag(d: i64) -> u64 {
    ((d << 1) ^ (d >> 63)) as u64
}

#[inline]
pub(crate) const fn unzigzag(z: u64) -> i64 {
    ((z >> 1) as i64) ^ -((z & 1) as i64)
}

/// Bucket widths of the delta code before the full width of the type.
const BUCKETS: [u8; 5] = [7, 9, 12, 20, 32];

/// The delta code for a `width`-bit type: `0` for a zero; otherwise `1` per bucket skipped, then `0` and the
/// zigzag value in the bucket's width (7, 9, 12, 20 or 32 bits, buckets at or above the type's width are left
/// out), or, after a `1` for every bucket, the value at the full width with no `0` in between.
pub(crate) fn put_delta(sink: &mut impl BitSink, z: u64, width: u8) -> Result<(), Error> {
    if z == 0 {
        return sink.put_bit(false);
    }
    sink.put_bit(true)?;
    for &bucket in BUCKETS.iter().filter(|&&b| b < width) {
        if z < (1u64 << bucket) {
            sink.put_bit(false)?;
            return sink.put_bits(bucket, z);
        }
        sink.put_bit(true)?;
    }
    sink.put_bits(width, z)
}

pub(crate) fn get_delta(rd: &mut BufReader<'_>, width: u8) -> Result<u64, Error> {
    if !rd.read_bool()? {
        return Ok(0);
    }
    for &bucket in BUCKETS.iter().filter(|&&b| b < width) {
        if !rd.read_bool()? {
            return rd.read_un64(bucket);
        }
    }
    rd.read_un64(width)
}

/// `width` low bits set.
#[inline]
pub(crate) const fn mask(width: u8) -> u64 {
    if width >= 64 {
        u64::MAX
    } else {
        (1u64 << width) - 1
    }
}

/// `bits` as a `width`-bit two's complement number.
#[inline]
pub(crate) const fn sign_extend(bits: u64, width: u8) -> i64 {
    let shift = 64 - width as u32;
    ((bits << shift) as i64) >> shift
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zigzag_round_trip() {
        for d in [0i64, 1, -1, 2, -2, 63, -63, 64, -64, i64::MAX, i64::MIN] {
            assert_eq!(unzigzag(zigzag(d)), d);
        }
        assert_eq!(zigzag(0), 0);
        assert_eq!(zigzag(-1), 1);
        assert_eq!(zigzag(1), 2);
        assert_eq!(zigzag(-64), 127);
        assert_eq!(zigzag(64), 128);
    }

    #[test]
    fn sign_extend_widths() {
        assert_eq!(sign_extend(0xFF, 8), -1);
        assert_eq!(sign_extend(0x7F, 8), 127);
        assert_eq!(sign_extend(0x80, 8), -128);
        assert_eq!(sign_extend(u64::MAX, 64), -1);
        assert_eq!(sign_extend(1, 64), 1);
        assert_eq!(mask(8), 0xFF);
        assert_eq!(mask(64), u64::MAX);
    }

    #[test]
    fn delta_code_bits() {
        // (zigzag value, width) -> bits written
        let cases: [(u64, u8, &str); 9] = [
            (0, 32, "0"),
            (1, 32, "10 0000001"),
            (127, 32, "10 1111111"),
            (128, 32, "110 010000000"),
            (511, 32, "110 111111111"),
            (512, 32, "1110 001000000000"),
            (4096, 32, "11110 00000001000000000000"),
            (1 << 20, 32, "11111 00000000000100000000000000000000"),
            (200, 8, "11 11001000"), // u8: the 7-bit bucket, then the full 8 bits
        ];
        for (z, width, expected) in cases {
            let mut buf = [0u8; 16];
            let mut wr = BufWriter::new(&mut buf);
            put_delta(&mut wr, z, width).unwrap();
            let (byte, bit) = wr.pos();
            let bits_written = byte * 8 + (7 - bit as usize);
            let bytes = wr.finish().unwrap();
            let mut s = String::new();
            for i in 0..bits_written {
                s.push(if bytes[i / 8] & (0x80 >> (i % 8)) != 0 {
                    '1'
                } else {
                    '0'
                });
            }
            assert_eq!(s, expected.replace(' ', ""), "z = {z}, width = {width}");
            let mut rd = BufReader::new(bytes);
            assert_eq!(get_delta(&mut rd, width).unwrap(), z);
        }
    }
}

//! [ArrayString]: an owned, no-alloc string with a fixed, compile-time capacity (SW-57).

use crate::max_size::MaxSize;
use crate::{BufReader, BufWriter, DeserializeShrinkWrap, ElementSize, Error, SerializeShrinkWrap};
use core::fmt::{Debug, Display, Formatter};
use core::ops::Deref;

#[cfg(feature = "std")]
use crate::{BufWriterOwned, DeserializeShrinkWrapOwned, SerializeShrinkWrapOwned};

/// An owned string backed by a fixed-size `[u8; N]`, for places an owned no-alloc value is needed (a
/// `value_on_changed` field of a string property in a `no_alloc` server, a settings value): same bytes on the wire
/// as `&str` / `String`, so a field can change between them without changing a single byte.
///
/// Serializing one longer than `N` bytes fails with [Error::MaxLenExceeded], and so does deserializing one, and
/// [push_str](Self::push_str) and [try_from](Self::try_from) too: a value is never cut.
///
/// ```
/// use shrink_wrap::ArrayString;
///
/// let mut s: ArrayString<8> = ArrayString::new();
/// s.push_str("abc").unwrap();
/// assert_eq!(s.as_str(), "abc");
/// assert_eq!(s.push_str("defghij"), Err(shrink_wrap::Error::MaxLenExceeded));
///
/// let s2: ArrayString<8> = ArrayString::try_from("abc").unwrap();
/// assert_eq!(s, s2);
/// ```
#[derive(Copy, Clone)]
pub struct ArrayString<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> ArrayString<N> {
    /// Maximum number of bytes this value can hold.
    pub const CAPACITY: usize = N;

    /// An empty string.
    pub const fn new() -> Self {
        Self {
            buf: [0u8; N],
            len: 0,
        }
    }

    pub fn as_str(&self) -> &str {
        // SAFETY: buf[..len] only ever receives bytes copied from a &str, through push_str, so it stays valid
        // UTF-8 (a &str slice boundary is never split: push_str copies whole `s.as_bytes()` at a time).
        unsafe { core::str::from_utf8_unchecked(&self.buf[..self.len]) }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Appends `s`, [Error::MaxLenExceeded] and no change if it would not fit.
    pub fn push_str(&mut self, s: &str) -> Result<(), Error> {
        let new_len = self.len + s.len();
        if new_len > N {
            return Err(Error::MaxLenExceeded);
        }
        self.buf[self.len..new_len].copy_from_slice(s.as_bytes());
        self.len = new_len;
        Ok(())
    }
}

impl<const N: usize> Default for ArrayString<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> TryFrom<&str> for ArrayString<N> {
    type Error = Error;

    fn try_from(s: &str) -> Result<Self, Error> {
        let mut out = Self::new();
        out.push_str(s)?;
        Ok(out)
    }
}

impl<const N: usize> Deref for ArrayString<N> {
    type Target = str;

    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl<const N: usize> Debug for ArrayString<N> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        Debug::fmt(self.as_str(), f)
    }
}

impl<const N: usize> Display for ArrayString<N> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        Display::fmt(self.as_str(), f)
    }
}

impl<const N: usize> PartialEq for ArrayString<N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl<const N: usize> Eq for ArrayString<N> {}

#[cfg(feature = "defmt")]
impl<const N: usize> defmt::Format for ArrayString<N> {
    fn format(&self, fmt: defmt::Formatter) {
        defmt::write!(fmt, "{}", self.as_str())
    }
}

impl<const N: usize> SerializeShrinkWrap for ArrayString<N> {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;
    const MAX_SIZE: Option<MaxSize> = MaxSize::list(Some(MaxSize::bytes(1)), N);

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.write_str(self.as_str())
    }
}

#[cfg(feature = "std")]
impl<const N: usize> SerializeShrinkWrapOwned for ArrayString<N> {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn ser_shrink_wrap_owned(&self, wr: &mut BufWriterOwned) -> Result<(), Error> {
        wr.write_str(self.as_str())
    }
}

impl<'i, const N: usize> DeserializeShrinkWrap<'i> for ArrayString<N> {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        let s = rd.read_str()?;
        Self::try_from(s)
    }
}

#[cfg(feature = "std")]
impl<const N: usize> DeserializeShrinkWrapOwned for ArrayString<N> {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, Error> {
        let s = rd.read_str()?;
        Self::try_from(s)
    }
}

#[cfg(test)]
mod tests {
    use crate::{ArrayString, BufReader, BufWriter, DeserializeShrinkWrap, Error};

    #[test]
    fn new_is_empty() {
        let s: ArrayString<4> = ArrayString::new();
        assert_eq!(s.as_str(), "");
        assert_eq!(ArrayString::<4>::CAPACITY, 4);
    }

    #[test]
    fn push_str_within_capacity() {
        let mut s: ArrayString<4> = ArrayString::new();
        s.push_str("ab").unwrap();
        s.push_str("cd").unwrap();
        assert_eq!(s.as_str(), "abcd");
    }

    #[test]
    fn push_str_too_long() {
        let mut s: ArrayString<4> = ArrayString::new();
        assert_eq!(s.push_str("abcde"), Err(Error::MaxLenExceeded));
        assert_eq!(s.as_str(), "");
    }

    #[test]
    fn try_from_too_long() {
        assert_eq!(
            ArrayString::<4>::try_from("abcde"),
            Err(Error::MaxLenExceeded)
        );
    }

    #[test]
    fn same_bytes_as_str() {
        let s: ArrayString<8> = ArrayString::try_from("abc").unwrap();
        let mut buf_array = [0u8; 16];
        let array_bytes = {
            let mut wr = BufWriter::new(&mut buf_array);
            wr.write(&s).unwrap();
            wr.finish_and_take().unwrap()
        };

        let mut buf_str = [0u8; 16];
        let str_bytes = {
            let mut wr = BufWriter::new(&mut buf_str);
            wr.write(&"abc").unwrap();
            wr.finish_and_take().unwrap()
        };

        assert_eq!(array_bytes, str_bytes);
    }

    #[test]
    fn round_trip() {
        let s: ArrayString<8> = ArrayString::try_from("abc").unwrap();
        let mut buf = [0u8; 16];
        let mut wr = BufWriter::new(&mut buf);
        wr.write(&s).unwrap();
        let bytes = wr.finish_and_take().unwrap();

        let mut rd = BufReader::new(bytes);
        let s_des: ArrayString<8> = rd.read().unwrap();
        assert_eq!(s, s_des);
    }

    #[test]
    fn deserialize_too_long_errors() {
        let mut buf = [0u8; 16];
        let mut wr = BufWriter::new(&mut buf);
        wr.write(&"abcde").unwrap();
        let bytes = wr.finish_and_take().unwrap();

        let mut rd = BufReader::new(bytes);
        assert_eq!(
            ArrayString::<4>::des_shrink_wrap(&mut rd),
            Err(Error::MaxLenExceeded)
        );
    }
}

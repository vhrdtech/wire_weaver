//! [ArrayVec]: an owned, no-alloc list with a fixed, compile-time capacity (SW-57).

use crate::max_size::{MaxSize, write_max_size};
use crate::{BufReader, BufWriter, DeserializeShrinkWrap, ElementSize, Error, SerializeShrinkWrap};
use core::fmt::{Debug, Formatter};
use core::mem::MaybeUninit;
use core::ops::{Deref, DerefMut};

#[cfg(feature = "std")]
use crate::{BufWriterOwned, DeserializeShrinkWrapOwned, SerializeShrinkWrapOwned};

/// An owned list backed by a fixed-size array, for places an owned no-alloc value is needed (a
/// `value_on_changed` field in a `no_alloc` server, a settings value): same bytes on the wire as `Vec<T>`, so a
/// field can change between them without changing a single byte.
///
/// Serializing one with more than `N` elements fails with [Error::MaxLenExceeded], and so does deserializing one,
/// and [push](Self::push) too: a value is never cut.
///
/// ```
/// use shrink_wrap::ArrayVec;
///
/// let mut v: ArrayVec<u8, 4> = ArrayVec::new();
/// v.push(1).unwrap();
/// v.push(2).unwrap();
/// assert_eq!(v.as_slice(), &[1, 2]);
/// v.push(3).unwrap();
/// v.push(4).unwrap();
/// assert_eq!(v.push(5), Err(shrink_wrap::Error::MaxLenExceeded));
/// ```
pub struct ArrayVec<T, const N: usize> {
    buf: [MaybeUninit<T>; N],
    len: usize,
}

impl<T, const N: usize> ArrayVec<T, N> {
    /// Maximum number of elements this value can hold.
    pub const CAPACITY: usize = N;

    /// An empty list.
    pub const fn new() -> Self {
        Self {
            buf: [const { MaybeUninit::uninit() }; N],
            len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Appends `value`, [Error::MaxLenExceeded] and no change if the list is already at capacity.
    pub fn push(&mut self, value: T) -> Result<(), Error> {
        if self.len >= N {
            return Err(Error::MaxLenExceeded);
        }
        self.buf[self.len] = MaybeUninit::new(value);
        self.len += 1;
        Ok(())
    }

    pub fn as_slice(&self) -> &[T] {
        // SAFETY: buf[..len] is only ever written by push / des_shrink_wrap, never past len.
        unsafe { core::slice::from_raw_parts(self.buf.as_ptr().cast(), self.len) }
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        // SAFETY: see as_slice.
        unsafe { core::slice::from_raw_parts_mut(self.buf.as_mut_ptr().cast(), self.len) }
    }
}

impl<T, const N: usize> Drop for ArrayVec<T, N> {
    fn drop(&mut self) {
        for item in &mut self.buf[..self.len] {
            // SAFETY: buf[..len] is always initialized.
            unsafe { item.assume_init_drop() };
        }
    }
}

impl<T, const N: usize> Default for ArrayVec<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> Deref for ArrayVec<T, N> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T, const N: usize> DerefMut for ArrayVec<T, N> {
    fn deref_mut(&mut self) -> &mut [T] {
        self.as_mut_slice()
    }
}

impl<T: Clone, const N: usize> Clone for ArrayVec<T, N> {
    fn clone(&self) -> Self {
        let mut out = Self::new();
        for item in self.as_slice() {
            // self.len <= N already, so this cannot fail.
            let _ = out.push(item.clone());
        }
        out
    }
}

impl<T: Debug, const N: usize> Debug for ArrayVec<T, N> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        Debug::fmt(self.as_slice(), f)
    }
}

impl<T: PartialEq, const N: usize> PartialEq for ArrayVec<T, N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: Eq, const N: usize> Eq for ArrayVec<T, N> {}

#[cfg(feature = "defmt")]
impl<T: defmt::Format, const N: usize> defmt::Format for ArrayVec<T, N> {
    fn format(&self, fmt: defmt::Formatter) {
        defmt::write!(fmt, "{=[?]}", self.as_slice())
    }
}

impl<T: SerializeShrinkWrap, const N: usize> SerializeShrinkWrap for ArrayVec<T, N> {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;
    const MAX_SIZE: Option<MaxSize> = MaxSize::list(write_max_size::<T>(), N);

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.write_rev_len(self.len)?;
        for item in self.as_slice() {
            wr.write(item)?;
        }
        Ok(())
    }
}

#[cfg(feature = "std")]
impl<T: SerializeShrinkWrapOwned, const N: usize> SerializeShrinkWrapOwned for ArrayVec<T, N> {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn ser_shrink_wrap_owned(&self, wr: &mut BufWriterOwned) -> Result<(), Error> {
        wr.write_rev_len(self.len)?;
        for item in self.as_slice() {
            wr.write(item)?;
        }
        Ok(())
    }
}

impl<'i, T: DeserializeShrinkWrap<'i>, const N: usize> DeserializeShrinkWrap<'i>
    for ArrayVec<T, N>
{
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        let elements_count = rd.read_rev_len()?;
        if elements_count > N {
            return Err(Error::MaxLenExceeded);
        }
        let mut out = Self::new();
        for _ in 0..elements_count {
            let item: T = rd.read()?;
            // elements_count <= N already, so this cannot fail.
            let _ = out.push(item);
        }
        Ok(out)
    }
}

#[cfg(feature = "std")]
impl<T: DeserializeShrinkWrapOwned, const N: usize> DeserializeShrinkWrapOwned for ArrayVec<T, N> {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, Error> {
        let elements_count = rd.read_rev_len()?;
        if elements_count > N {
            return Err(Error::MaxLenExceeded);
        }
        let mut out = Self::new();
        for _ in 0..elements_count {
            let item: T = rd.read_owned()?;
            // elements_count <= N already, so this cannot fail.
            let _ = out.push(item);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use crate::{ArrayVec, BufReader, BufWriter, DeserializeShrinkWrap, Error};

    #[test]
    fn new_is_empty() {
        let v: ArrayVec<u8, 4> = ArrayVec::new();
        assert!(v.is_empty());
        assert_eq!(ArrayVec::<u8, 4>::CAPACITY, 4);
    }

    #[test]
    fn push_within_capacity() {
        let mut v: ArrayVec<u8, 4> = ArrayVec::new();
        v.push(1).unwrap();
        v.push(2).unwrap();
        assert_eq!(v.as_slice(), &[1, 2]);
    }

    #[test]
    fn push_too_long() {
        let mut v: ArrayVec<u8, 2> = ArrayVec::new();
        v.push(1).unwrap();
        v.push(2).unwrap();
        assert_eq!(v.push(3), Err(Error::MaxLenExceeded));
        assert_eq!(v.as_slice(), &[1, 2]);
    }

    #[test]
    fn same_bytes_as_vec() {
        let mut v: ArrayVec<u8, 8> = ArrayVec::new();
        v.push(1).unwrap();
        v.push(2).unwrap();
        v.push(3).unwrap();

        let mut buf_array = [0u8; 16];
        let array_bytes = {
            let mut wr = BufWriter::new(&mut buf_array);
            wr.write(&v).unwrap();
            wr.finish_and_take().unwrap()
        };

        let vec = vec![1u8, 2, 3];
        let mut buf_vec = [0u8; 16];
        let vec_bytes = {
            let mut wr = BufWriter::new(&mut buf_vec);
            wr.write(&vec).unwrap();
            wr.finish_and_take().unwrap()
        };

        assert_eq!(array_bytes, vec_bytes);
    }

    #[test]
    fn round_trip() {
        let mut v: ArrayVec<u8, 8> = ArrayVec::new();
        v.push(1).unwrap();
        v.push(2).unwrap();
        v.push(3).unwrap();

        let mut buf = [0u8; 16];
        let mut wr = BufWriter::new(&mut buf);
        wr.write(&v).unwrap();
        let bytes = wr.finish_and_take().unwrap();

        let mut rd = BufReader::new(bytes);
        let v_des: ArrayVec<u8, 8> = rd.read().unwrap();
        assert_eq!(v, v_des);
    }

    #[test]
    fn deserialize_too_long_errors() {
        let vec = vec![1u8, 2, 3, 4, 5];
        let mut buf = [0u8; 16];
        let mut wr = BufWriter::new(&mut buf);
        wr.write(&vec).unwrap();
        let bytes = wr.finish_and_take().unwrap();

        let mut rd = BufReader::new(bytes);
        assert_eq!(
            ArrayVec::<u8, 4>::des_shrink_wrap(&mut rd),
            Err(Error::MaxLenExceeded)
        );
    }

    #[test]
    fn drop_runs_for_each_element() {
        use std::rc::Rc;
        let counter = Rc::new(());
        {
            let mut v: ArrayVec<Rc<()>, 4> = ArrayVec::new();
            v.push(counter.clone()).unwrap();
            v.push(counter.clone()).unwrap();
            assert_eq!(Rc::strong_count(&counter), 3);
        }
        assert_eq!(Rc::strong_count(&counter), 1);
    }
}

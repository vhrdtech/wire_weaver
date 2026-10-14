//! [MaxLen]: a list or string with a maximum length (SW-19).

use crate::max_size::{MaxSize, write_max_size};
use crate::{
    BufReader, BufWriter, DeserializeShrinkWrap, DeserializeShrinkWrapOwned, ElementSize, Error,
    RefVec, SerializeShrinkWrap,
};
use core::ops::{Deref, DerefMut};

/// A list or string with at most `N` elements (bytes for a string): `MaxLen<RefVec<'i, T>, N>`,
/// `MaxLen<Vec<T>, N>`, `MaxLen<&'i str, N>` or `MaxLen<String, N>`.
///
/// On the wire it is exactly the value inside, so a bound can be added to, changed in or removed from a field
/// without changing a single byte. A longer value is never cut: serializing it fails with
/// [Error::MaxLenExceeded], and so does deserializing one, before any of its elements are read.
///
/// Its [MAX_SIZE](SerializeShrinkWrap::MAX_SIZE) is known if the elements' is, so the worst-case size of a type or a
/// message made of bounded parts is known at compile time (see [max_size](crate::max_size)).
///
/// ```
/// use shrink_wrap::prelude::*;
///
/// let data = [1u8, 2, 3];
/// let short: MaxLen<RefVec<'_, u8>, 4> = MaxLen::new(RefVec::new_bytes(&data)).unwrap();
/// let mut buf = [0u8; 16];
/// let bytes = short.to_ww_bytes(&mut buf).unwrap();
/// // same bytes as the RefVec alone
/// assert_eq!(bytes, RefVec::new_bytes(&data).to_ww_bytes(&mut [0u8; 16]).unwrap());
///
/// // read with a smaller bound: an error, not a truncated value
/// assert_eq!(
///     MaxLen::<RefVec<'_, u8>, 2>::from_ww_bytes(bytes).err(),
///     Some(ShrinkWrapError::MaxLenExceeded)
/// );
/// assert_eq!(
///     MaxLen::<RefVec<'_, u8>, 2>::new(RefVec::new_bytes(&data)).err(),
///     Some(ShrinkWrapError::MaxLenExceeded)
/// );
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct MaxLen<T, const N: usize>(T);

impl<T, const N: usize> MaxLen<T, N> {
    /// The maximum number of elements (bytes for a string).
    pub const MAX_LEN: usize = N;

    /// The value without a check: one that is too long fails to serialize instead. For a `const`, where
    /// [new](Self::new) can't be used.
    pub const fn new_unchecked(value: T) -> Self {
        MaxLen(value)
    }

    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T: LenBounded, const N: usize> MaxLen<T, N> {
    /// `value` if it has at most `N` elements, [Error::MaxLenExceeded] otherwise.
    pub fn new(value: T) -> Result<Self, Error> {
        if value.bounded_len() > N {
            return Err(Error::MaxLenExceeded);
        }
        Ok(MaxLen(value))
    }
}

impl<T, const N: usize> Deref for MaxLen<T, N> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

/// A value made longer than `N` through this fails to serialize.
impl<T, const N: usize> DerefMut for MaxLen<T, N> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

/// A list or string that [MaxLen] can bound: `RefVec`, `Vec`, `&str`, `String`.
pub trait LenBounded {
    /// Number of elements, bytes for a string.
    fn bounded_len(&self) -> usize;
}

/// [MAX_SIZE](SerializeShrinkWrap::MAX_SIZE) of one element of a [LenBounded] list as written in it, for the
/// `MAX_SIZE` of [MaxLen]. A separate trait, so that bounding a list of types that are only
/// [SerializeShrinkWrapOwned](crate::SerializeShrinkWrapOwned) works too.
pub trait ItemMaxSize {
    const ITEM_MAX_SIZE: Option<MaxSize>;
}

impl<T> LenBounded for RefVec<'_, T> {
    fn bounded_len(&self) -> usize {
        self.len()
    }
}

impl<T: SerializeShrinkWrap> ItemMaxSize for RefVec<'_, T> {
    const ITEM_MAX_SIZE: Option<MaxSize> = write_max_size::<T>();
}

impl LenBounded for &str {
    fn bounded_len(&self) -> usize {
        self.len()
    }
}

impl ItemMaxSize for &str {
    const ITEM_MAX_SIZE: Option<MaxSize> = Some(MaxSize::bytes(1));
}

#[cfg(feature = "std")]
impl<T> LenBounded for Vec<T> {
    fn bounded_len(&self) -> usize {
        self.len()
    }
}

#[cfg(feature = "std")]
impl<T: SerializeShrinkWrap> ItemMaxSize for Vec<T> {
    const ITEM_MAX_SIZE: Option<MaxSize> = write_max_size::<T>();
}

#[cfg(feature = "std")]
impl LenBounded for String {
    fn bounded_len(&self) -> usize {
        self.len()
    }
}

#[cfg(feature = "std")]
impl ItemMaxSize for String {
    const ITEM_MAX_SIZE: Option<MaxSize> = Some(MaxSize::bytes(1));
}

impl<T: SerializeShrinkWrap + LenBounded + ItemMaxSize, const N: usize> SerializeShrinkWrap
    for MaxLen<T, N>
{
    const ELEMENT_SIZE: ElementSize = T::ELEMENT_SIZE;
    const PUSHES_LEN: bool = T::PUSHES_LEN;
    const MAX_SIZE: Option<MaxSize> = MaxSize::list(T::ITEM_MAX_SIZE, N);

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        if self.0.bounded_len() > N {
            return Err(Error::MaxLenExceeded);
        }
        self.0.ser_shrink_wrap(wr)
    }
}

#[cfg(feature = "std")]
impl<T: crate::SerializeShrinkWrapOwned + LenBounded, const N: usize>
    crate::SerializeShrinkWrapOwned for MaxLen<T, N>
{
    const ELEMENT_SIZE: ElementSize = T::ELEMENT_SIZE;

    fn ser_shrink_wrap_owned(&self, wr: &mut crate::BufWriterOwned) -> Result<(), Error> {
        if self.0.bounded_len() > N {
            return Err(Error::MaxLenExceeded);
        }
        self.0.ser_shrink_wrap_owned(wr)
    }
}

/// Every type [LenBounded] is for starts with its length at the back of the buffer: read it from a copy of the
/// reader and fail before the value itself is read (or allocated).
fn check_len(rd: &BufReader<'_>, max_len: usize) -> Result<(), Error> {
    let mut peek = *rd;
    if peek.read_rev_len()? > max_len {
        return Err(Error::MaxLenExceeded);
    }
    Ok(())
}

impl<'i, T: DeserializeShrinkWrap<'i> + LenBounded, const N: usize> DeserializeShrinkWrap<'i>
    for MaxLen<T, N>
{
    const ELEMENT_SIZE: ElementSize = T::ELEMENT_SIZE;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        check_len(rd, N)?;
        Ok(MaxLen(T::des_shrink_wrap(rd)?))
    }
}

impl<T: DeserializeShrinkWrapOwned + LenBounded, const N: usize> DeserializeShrinkWrapOwned
    for MaxLen<T, N>
{
    const ELEMENT_SIZE: ElementSize = T::ELEMENT_SIZE;

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, Error> {
        check_len(rd, N)?;
        Ok(MaxLen(T::des_shrink_wrap_owned(rd)?))
    }
}

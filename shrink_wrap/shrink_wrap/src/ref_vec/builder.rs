//! Writing a [RefVec](super::RefVec) in place, element by element (SW-61).

use core::borrow::Borrow;
use core::marker::PhantomData;

use crate::buf_writer::{RevPos, UnsizedBuilder};
use crate::traits::ElementSize;
use crate::{BufWriter, Error, SerializeShrinkWrap};

/// no_std and no-alloc writer of a [RefVec](super::RefVec) in place, into a [BufWriter] that may hold other data
/// too, without a slice of the elements: from an iterator, a sensor loop, a nested builder.
///
/// [new](Self::new) reserves the element count at the back of the buffer, each [push](Self::push) writes one
/// element right away as the list would, and [finish](Self::finish) fills in the count. The bytes are the same as
/// those of `RefVec::Slice` with the same elements, wherever it is written: the whole buffer, a field of a struct
/// written by hand, an element of another list.
///
/// ```
/// use shrink_wrap::prelude::*;
/// use shrink_wrap::RefVecBuilder;
///
/// let mut buf = [0u8; 32];
/// let mut wr = BufWriter::new(&mut buf);
/// let mut samples = RefVecBuilder::<u16>::new(&mut wr).unwrap();
/// for i in 0..3u16 {
///     samples.push(&mut wr, &(i * 0x101)).unwrap(); // e.g. read from an ADC
/// }
/// samples.finish(&mut wr).unwrap();
/// let bytes = wr.finish_and_take().unwrap();
/// assert_eq!(bytes, &[0x00, 0x00, 0x01, 0x01, 0x02, 0x02, 0x03]);
/// assert_eq!(bytes, RefVec::Slice { slice: &[0u16, 0x101, 0x202] }.to_ww_bytes(&mut [0u8; 32]).unwrap());
/// ```
///
/// A push that fails (out of buffer space, an element that fails to serialize) returns the error and leaves the
/// writer as it was before it: the list holds the elements pushed so far and can still be finished. Without
/// [finish](Self::finish) the count stays 0 while the elements are already in the buffer, so a reader sees
/// garbage: always finish a builder, or drop the whole buffer.
#[must_use = "the element count is only written by finish()"]
pub struct RefVecBuilder<T> {
    count_pos: RevPos,
    count: usize,
    max_len: usize,
    _element: PhantomData<fn(&T)>,
}

impl<T: SerializeShrinkWrap> RefVecBuilder<T> {
    /// Start a list at the current position of `wr`. A `RefVec` has no size of its own, so this is the same for a
    /// list that is the whole buffer, a field of a struct or an element of another list.
    pub fn new(wr: &mut BufWriter<'_>) -> Result<Self, Error> {
        Self::with_max_len(wr, usize::MAX)
    }

    /// [new](Self::new) for a `MaxLen<RefVec<'_, T>, N>` (pass `N`, or `MaxLen::<RefVec<'_, T>, N>::MAX_LEN`):
    /// a push after `max_len` elements fails with [Error::MaxLenExceeded] and writes nothing.
    pub fn with_max_len(wr: &mut BufWriter<'_>, max_len: usize) -> Result<Self, Error> {
        let count_pos = wr.write_rev_len(0)?;
        Ok(RefVecBuilder {
            count_pos,
            count: 0,
            max_len,
            _element: PhantomData,
        })
    }

    /// Number of elements pushed so far.
    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Write one element, as the list writes it ([BufWriter::write]: with its size if `T` is `Unsized`).
    pub fn push(&mut self, wr: &mut BufWriter<'_>, value: &T) -> Result<(), Error> {
        self.push_with(wr, |wr| value.ser_shrink_wrap(wr))
    }

    /// Write one element with `f`, which writes it as [SerializeShrinkWrap::ser_shrink_wrap] of `T` would; the
    /// builder adds the size of an `Unsized` element around it. For an element that is itself a list: a nested
    /// `RefVecBuilder` started and finished inside `f`.
    pub fn push_with<'b>(
        &mut self,
        wr: &mut BufWriter<'b>,
        f: impl FnOnce(&mut BufWriter<'b>) -> Result<(), Error>,
    ) -> Result<(), Error> {
        if self.count >= self.max_len {
            return Err(Error::MaxLenExceeded);
        }
        if self.count >= u16::MAX as usize {
            // BufWriter keeps lengths as u16 until they are encoded
            return Err(Error::LenTooLong);
        }
        // an error of a write before this push is the caller's, not to be undone with this element
        wr.latched()?;
        let state = wr.save_state();
        match Self::write_element(wr, f) {
            Ok(()) => {
                self.count += 1;
                Ok(())
            }
            Err(e) => {
                // forward bytes and the lengths the element pushed are dropped, the count slot is below them;
                // so is an error of a write_*_latch of the element that is still kept in the writer
                wr.restore_state(state);
                let _ = wr.latched();
                Err(e)
            }
        }
    }

    fn write_element<'b>(
        wr: &mut BufWriter<'b>,
        f: impl FnOnce(&mut BufWriter<'b>) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let size = if matches!(T::ELEMENT_SIZE, ElementSize::Unsized) {
            Some(UnsizedBuilder::new(wr)?)
        } else {
            None
        };
        f(wr)?;
        // a write_*_latch that failed inside f is this element's error
        wr.latched()?;
        if let Some(size) = size {
            size.finish(wr)?;
        }
        Ok(())
    }

    /// [push](Self::push) every item, stopping at the first error (the items before it stay in the list).
    pub fn extend<I>(&mut self, wr: &mut BufWriter<'_>, items: I) -> Result<(), Error>
    where
        I: IntoIterator,
        I::Item: Borrow<T>,
    {
        for item in items {
            self.push(wr, item.borrow())?;
        }
        Ok(())
    }

    /// Write the element count.
    pub fn finish(self, wr: &mut BufWriter<'_>) -> Result<(), Error> {
        wr.update_rev_len(self.count_pos, self.count)
    }
}

/// [RefVecBuilder] for a [BufWriterOwned](crate::BufWriterOwned), writing
/// [SerializeShrinkWrapOwned](crate::SerializeShrinkWrapOwned) elements: same bytes.
///
/// A push that fails leaves the writer as it was before it, as with [RefVecBuilder].
#[cfg(feature = "std")]
#[must_use = "the element count is only written by finish()"]
pub struct RefVecBuilderOwned<T> {
    count_pos: crate::buf_writer_owned::RevPos,
    count: usize,
    max_len: usize,
    _element: PhantomData<fn(&T)>,
}

#[cfg(feature = "std")]
impl<T: crate::SerializeShrinkWrapOwned> RefVecBuilderOwned<T> {
    /// See [RefVecBuilder::new].
    pub fn new(wr: &mut crate::BufWriterOwned) -> Result<Self, Error> {
        Self::with_max_len(wr, usize::MAX)
    }

    /// See [RefVecBuilder::with_max_len].
    pub fn with_max_len(wr: &mut crate::BufWriterOwned, max_len: usize) -> Result<Self, Error> {
        let count_pos = wr.write_rev_len(0)?;
        Ok(RefVecBuilderOwned {
            count_pos,
            count: 0,
            max_len,
            _element: PhantomData,
        })
    }

    /// Number of elements pushed so far.
    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// See [RefVecBuilder::push].
    pub fn push(&mut self, wr: &mut crate::BufWriterOwned, value: &T) -> Result<(), Error> {
        self.push_with(wr, |wr| value.ser_shrink_wrap_owned(wr))
    }

    /// See [RefVecBuilder::push_with].
    pub fn push_with(
        &mut self,
        wr: &mut crate::BufWriterOwned,
        f: impl FnOnce(&mut crate::BufWriterOwned) -> Result<(), Error>,
    ) -> Result<(), Error> {
        if self.count >= self.max_len {
            return Err(Error::MaxLenExceeded);
        }
        if self.count >= u32::MAX as usize {
            return Err(Error::LenTooLong);
        }
        let mark = wr.mark();
        match Self::write_element(wr, f) {
            Ok(()) => {
                self.count += 1;
                Ok(())
            }
            Err(e) => {
                wr.rollback(mark);
                Err(e)
            }
        }
    }

    fn write_element(
        wr: &mut crate::BufWriterOwned,
        f: impl FnOnce(&mut crate::BufWriterOwned) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let size = if matches!(T::ELEMENT_SIZE, ElementSize::Unsized) {
            Some(crate::buf_writer_owned::UnsizedBuilderOwned::new(wr)?)
        } else {
            None
        };
        f(wr)?;
        if let Some(size) = size {
            size.finish(wr)?;
        }
        Ok(())
    }

    /// See [RefVecBuilder::extend].
    pub fn extend<I>(&mut self, wr: &mut crate::BufWriterOwned, items: I) -> Result<(), Error>
    where
        I: IntoIterator,
        I::Item: Borrow<T>,
    {
        for item in items {
            self.push(wr, item.borrow())?;
        }
        Ok(())
    }

    /// See [RefVecBuilder::finish].
    pub fn finish(self, wr: &mut crate::BufWriterOwned) -> Result<(), Error> {
        wr.update_rev_len(self.count_pos, self.count)
    }
}

/// Writer of a `RefVec<'_, u8>` in place from bytes written by other means: [BufWriter::write_raw_slice],
/// [write_u8](BufWriter::write_u8), even other shrink_wrap values (the list is then a byte blob holding them).
///
/// [new](Self::new) reserves the count and aligns to a byte, [finish](Self::finish) encodes the lengths pushed
/// since (by values written into the blob), aligns to a byte and writes the number of bytes as the count. A blob
/// of plain bytes is the same as `RefVec::new_bytes` with them, an empty one too. For a list of `u8` pushed one by
/// one, [RefVecBuilder] works as well. Like it, a builder that is not finished leaves a corrupt list.
#[must_use = "the byte count is only written by finish()"]
pub struct RefVecU8Builder {
    element_count_pos: RevPos,
    byte_pos: usize,
    before: crate::buf_writer::BufWriterState,
}

impl RefVecU8Builder {
    pub fn new(wr: &mut BufWriter<'_>) -> Result<Self, Error> {
        let before = wr.save_state();
        wr.align_byte();
        let element_count_pos = wr.write_rev_len(0)?;
        let byte_pos = wr.pos().0;
        Ok(Self {
            element_count_pos,
            byte_pos,
            before,
        })
    }

    pub fn finish(self, wr: &mut BufWriter<'_>) -> Result<(), Error> {
        if wr.pos() == (self.byte_pos, 7) && wr.rev_len_pos() == self.element_count_pos {
            // nothing written: undo the alignment, an empty RefVec<u8> doesn't align (the slot gets the same place)
            wr.restore_state(self.before);
            wr.write_rev_len(0)?;
            return Ok(());
        }
        // values in the blob might have written several nib16_rev's as well, encode and place them after the data
        wr.encode_len_fifo(wr.rev_len_pos(), self.element_count_pos)?;
        // e.g., enum, only one nib discriminant is written => need to align
        wr.align_byte();
        let size_bytes = wr.pos().0 - self.byte_pos;
        wr.update_rev_len(self.element_count_pos, size_bytes)
    }
}

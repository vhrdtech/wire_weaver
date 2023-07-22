use crate::error::{Error, Result};
// use cobs::{EncoderState, PushResult};
use core::marker::PhantomData;
use core::ops::Index;
use core::ops::IndexMut;

#[cfg(feature = "heapless")]
pub use heapless_vec::*;

#[cfg(feature = "use-std")]
pub use std_vec::*;

#[cfg(feature = "alloc")]
pub use alloc_vec::*;

#[cfg(feature = "alloc")]
extern crate alloc;

/// The serialization Flavor trait
///
/// This is used as the primary way to encode serialized data into some kind of buffer,
/// or modify that data in a middleware style pattern.
///
/// See the module level docs for an example of how flavors are used.
pub trait Flavor {
    /// The `Output` type is what this storage "resolves" to when the serialization is complete,
    /// such as a slice or a Vec of some sort.
    type Output;

    /// The try_extend() trait method can be implemented when there is a more efficient way of processing
    /// multiple bytes at once, such as copying a slice to the output, rather than iterating over one byte
    /// at a time.
    #[inline]
    fn try_extend(&mut self, data: &[u8]) -> Result<()> {
        data.iter().try_for_each(|d| self.try_push(*d))
    }

    /// The try_push() trait method can be used to push a single byte to be modified and/or stored
    fn try_push(&mut self, data: u8) -> Result<()>;

    /// Finalize the serialization process
    fn finalize(self) -> Result<Self::Output>;
}

/// The `Slice` flavor is a storage flavor, storing the serialized (or otherwise modified) bytes into a plain
/// `[u8]` slice. The `Slice` flavor resolves into a sub-slice of the original slice buffer.
pub struct Slice<'a> {
    start: *mut u8,
    cursor: *mut u8,
    end: *mut u8,
    _pl: PhantomData<&'a [u8]>,
}

impl<'a> Slice<'a> {
    /// Create a new `Slice` flavor from a given backing buffer
    pub fn new(buf: &'a mut [u8]) -> Self {
        let ptr = buf.as_mut_ptr();
        Slice {
            start: ptr,
            cursor: ptr,
            end: unsafe { ptr.add(buf.len()) },
            _pl: PhantomData,
        }
    }
}

impl<'a> Flavor for Slice<'a> {
    type Output = &'a mut [u8];

    #[inline(always)]
    fn try_push(&mut self, b: u8) -> Result<()> {
        if self.cursor == self.end {
            Err(Error::SerializeBufferFull)
        } else {
            unsafe {
                self.cursor.write(b);
                self.cursor = self.cursor.add(1);
            }
            Ok(())
        }
    }

    #[inline(always)]
    fn try_extend(&mut self, b: &[u8]) -> Result<()> {
        let remain = (self.end as usize) - (self.cursor as usize);
        let blen = b.len();
        if blen > remain {
            Err(Error::SerializeBufferFull)
        } else {
            unsafe {
                core::ptr::copy_nonoverlapping(b.as_ptr(), self.cursor, blen);
                self.cursor = self.cursor.add(blen);
            }
            Ok(())
        }
    }

    fn finalize(self) -> Result<Self::Output> {
        let used = (self.cursor as usize) - (self.start as usize);
        let sli = unsafe { core::slice::from_raw_parts_mut(self.start, used) };
        Ok(sli)
    }
}

impl<'a> Index<usize> for Slice<'a> {
    type Output = u8;

    fn index(&self, idx: usize) -> &u8 {
        let len = (self.end as usize) - (self.start as usize);
        assert!(idx < len);
        unsafe { &*self.start.add(idx) }
    }
}

impl<'a> IndexMut<usize> for Slice<'a> {
    fn index_mut(&mut self, idx: usize) -> &mut u8 {
        let len = (self.end as usize) - (self.start as usize);
        assert!(idx < len);
        unsafe { &mut *self.start.add(idx) }
    }
}

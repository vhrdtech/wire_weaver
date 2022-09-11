use crate::serdes::bit_buf::BitBufMut;
use crate::serdes::traits::{DeserializeBytes, SerializeBytes};
use crate::serdes::{BitBuf, NibbleBuf, NibbleBufMut};
use core::ptr::copy_nonoverlapping;

/// Buffer reader that treats input as a stream of bytes
///
/// Use `rd` as short name: let mut rd = Buf::new(..);
#[derive(Copy, Clone)]
pub struct Buf<'i> {
    buf: &'i [u8],
    // Next byte to read
    idx: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Error {
    OutOfBounds,
    // TODO: replace by one common error in ll-buf crate
    NibbleBufError(crate::serdes::nibble_buf::Error),
    // UnalignedAccess,
}

impl From<crate::serdes::nibble_buf::Error> for Error {
    fn from(e: crate::serdes::nibble_buf::Error) -> Self {
        Error::NibbleBufError(e)
    }
}

impl<'i> Buf<'i> {
    pub fn new(buf: &'i [u8]) -> Self {
        Buf { buf, idx: 0 }
    }

    pub fn get_bit_buf(&mut self, byte_count: usize) -> Result<BitBuf<'i>, Error> {
        if self.bytes_left() < byte_count {
            return Err(Error::OutOfBounds);
        }
        let bit_buf = BitBuf::new_all(unsafe {
            &*core::ptr::slice_from_raw_parts(
                self.buf.as_ptr().offset(self.idx as isize),
                byte_count,
            )
        });
        self.idx += byte_count;

        Ok(bit_buf)
    }

    pub fn get_nibble_buf(&mut self, byte_count: usize) -> Result<NibbleBuf<'i>, Error> {
        if self.bytes_left() < byte_count {
            return Err(Error::OutOfBounds);
        }
        let nibble_buf = NibbleBuf::new_all(unsafe {
            &*core::ptr::slice_from_raw_parts(
                self.buf.as_ptr().offset(self.idx as isize),
                byte_count,
            )
        });
        self.idx += byte_count;

        Ok(nibble_buf)
    }

    pub fn byte_pos(&self) -> usize {
        self.idx
    }

    pub fn bytes_left(&self) -> usize {
        if !self.is_at_end() {
            self.buf.len() - self.idx
        } else {
            0
        }
    }

    pub fn is_at_end(&self) -> bool {
        self.idx >= self.buf.len()
    }

    pub fn get_u8(&mut self) -> Result<u8, Error> {
        if self.bytes_left() < 1 {
            return Err(Error::OutOfBounds);
        }
        let val = unsafe { *self.buf.get_unchecked(self.idx) };
        self.idx += 1;
        Ok(val)
    }

    pub fn get_u16_be(&mut self) -> Result<u16, Error> {
        if self.bytes_left() < 2 {
            return Err(Error::OutOfBounds);
        }
        let mut bytes = [0u8; 2];
        unsafe {
            copy_nonoverlapping(
                self.buf.as_ptr().offset(self.idx as isize),
                bytes.as_mut_ptr(),
                2,
            );
        }
        let val = u16::from_be_bytes(bytes);
        self.idx += 2;
        Ok(val)
    }

    pub fn get_u16_le(&mut self) -> Result<u16, Error> {
        if self.bytes_left() < 2 {
            return Err(Error::OutOfBounds);
        }
        let mut bytes = [0u8; 2];
        unsafe {
            copy_nonoverlapping(
                self.buf.as_ptr().offset(self.idx as isize),
                bytes.as_mut_ptr(),
                2,
            );
        }
        let val = u16::from_le_bytes(bytes);
        self.idx += 2;
        Ok(val)
    }

    pub fn des_bytes<'di, T: DeserializeBytes<'i>>(&'di mut self) -> Result<T, T::Error> {
        T::des_bytes(self)
    }
}

/// Buffer writer that treats input as a stream of bytes
///
/// Use `wr` as short name: let mut wr = BufMut::new(..);
pub struct BufMut<'i> {
    buf: &'i mut [u8],
    // Next byte to write
    idx: usize,
}

impl<'i> BufMut<'i> {
    pub fn new(buf: &'i mut [u8]) -> Self {
        BufMut { buf, idx: 0 }
    }

    pub fn as_bit_buf<E, F>(&mut self, f: F) -> Result<(), E>
    where
        E: From<crate::serdes::bit_buf::Error>,
        F: Fn(&mut BitBufMut) -> Result<(), E>,
    {
        let len_bits = self.buf.len() * 8;
        let mut bit_buf = BitBufMut {
            buf: self.buf,
            len_bits,
            idx: self.idx,
            bit_idx: 0,
        };
        f(&mut bit_buf)?;
        if bit_buf.bit_idx != 0 {
            return Err(E::from(crate::serdes::bit_buf::Error::UnalignedAccess));
        }
        self.idx = bit_buf.idx;
        Ok(())
    }

    pub fn as_nibble_buf<E, F>(&mut self, f: F) -> Result<(), E>
    where
        E: From<crate::serdes::nibble_buf::Error>,
        F: Fn(&mut NibbleBufMut) -> Result<(), E>,
    {
        let len_nibbles = self.buf.len() * 2;
        let mut nibble_buf = NibbleBufMut {
            buf: self.buf,
            len_nibbles,
            idx: self.idx,
            is_at_byte_boundary: true,
        };
        f(&mut nibble_buf)?;
        if !nibble_buf.is_at_byte_boundary {
            return Err(E::from(crate::serdes::nibble_buf::Error::UnalignedAccess));
        }
        self.idx = nibble_buf.idx;
        Ok(())
    }

    pub fn byte_pos(&self) -> usize {
        self.idx
    }

    pub fn bytes_left(&self) -> usize {
        if !self.is_at_end() {
            self.buf.len() - self.idx
        } else {
            0
        }
    }

    pub fn is_at_end(&self) -> bool {
        self.idx >= self.buf.len()
    }

    pub fn finish(self) -> (&'i mut [u8], usize) {
        (self.buf, self.idx)
    }

    pub fn put_u8(&mut self, val: u8) -> Result<(), Error> {
        if self.bytes_left() < 1 {
            return Err(Error::OutOfBounds);
        }
        unsafe {
            *self.buf.get_unchecked_mut(self.idx) = val;
        }
        self.idx += 1;
        Ok(())
    }

    pub fn put_u16_be(&mut self, val: u16) -> Result<(), Error> {
        if self.bytes_left() < 2 {
            return Err(Error::OutOfBounds);
        }
        let bytes = val.to_be_bytes();
        unsafe {
            copy_nonoverlapping(
                bytes.as_ptr(),
                self.buf.as_mut_ptr().offset(self.idx as isize),
                2,
            );
        }
        self.idx += 2;
        Ok(())
    }

    pub fn put_u16_le(&mut self, val: u16) -> Result<(), Error> {
        if self.bytes_left() < 2 {
            return Err(Error::OutOfBounds);
        }
        let bytes = val.to_le_bytes();
        unsafe {
            copy_nonoverlapping(
                bytes.as_ptr(),
                self.buf.as_mut_ptr().offset(self.idx as isize),
                2,
            );
        }
        self.idx += 2;
        Ok(())
    }

    /// Put any type that implements SerializeVlu4 into this buffer.
    pub fn put<E, T: SerializeBytes<Error = E>>(&mut self, t: &T) -> Result<(), E> {
        t.ser_bytes(self)
    }
}

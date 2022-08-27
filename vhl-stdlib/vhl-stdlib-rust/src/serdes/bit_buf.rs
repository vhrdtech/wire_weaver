// use thiserror::Error;
use crate::serdes::{DeserializeBits, NibbleBufMut};
use crate::serdes::traits::SerializeBits;

/// Buffer reader that treats input as a stream of bits
///
/// Use `brd` as short name: let mut brd = BitBuf::new(..);
#[derive(Copy, Clone)]
pub struct BitBuf<'i> {
    buf: &'i [u8],
    // Maximum number of bits to read (not whole buf might be available)
    len_bits: usize,
    // Next byte to read
    idx: usize,
    // Next bit to read, 0..=7
    bit_idx: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Error {
    /// Returned if read or write past end is attempted
    OutOfBounds,
    /// Returned by to_nibble_buf() if not at nibble boundary
    UnalignedAccess,
}

impl<'i> BitBuf<'i> {
    pub fn new(buf: &'i [u8], len_bits: usize) -> Result<Self, Error> {
        if len_bits >= buf.len() * 8 {
            return Err(Error::OutOfBounds);
        }
        Ok(BitBuf {
            buf,
            len_bits,
            idx: 0,
            bit_idx: 0
        })
    }

    pub fn new_all(buf: &'i [u8]) -> Self {
        BitBuf {
            buf,
            len_bits: buf.len() * 8,
            idx: 0,
            bit_idx: 0
        }
    }

    pub fn new_with_offset(buf: &'i [u8], offset_bits: usize, len_bits: usize) -> Result<Self, Error> {
        if (offset_bits + len_bits) > buf.len() * 8 || offset_bits > len_bits {
            return Err(Error::OutOfBounds);
        }
        Ok(BitBuf {
            buf,
            len_bits: len_bits + offset_bits,
            idx: offset_bits / 8,
            bit_idx: offset_bits % 8,
        })
    }

    pub fn bit_pos(&self) -> usize {
        self.idx * 8 + self.bit_idx
    }

    pub fn bits_left(&self) -> usize {
        if !self.is_at_end() {
            self.len_bits - self.bit_pos()
        } else {
            0
        }
    }

    pub fn is_at_end(&self) -> bool {
        self.bit_pos() >= self.len_bits
    }

    pub fn is_at_nibble_boundary(&self) -> bool {
        self.bit_idx == 4
    }

    pub fn is_at_byte_boundary(&self) -> bool {
        self.bit_idx == 0
    }

    pub fn get_bit(&mut self) -> Result<bool, Error> {
        Ok(self.get_up_to_8(1)? != 0)
    }

    pub fn get_up_to_8(&mut self, bit_count: usize) -> Result<u8, Error> {
        if self.bits_left() < bit_count {
            return Err(Error::OutOfBounds);
        }

        let left_in_current_byte = 8 - self.bit_idx;
        let requested_mask = 0b1111_1111 >> (8 - bit_count);
        if bit_count <= left_in_current_byte {
            let bits = unsafe {
                *self.buf.get_unchecked(self.idx) >> (left_in_current_byte - bit_count)
            } & requested_mask;
            self.bit_idx += bit_count;
            if self.bit_idx >= 8 {
                self.bit_idx = 0;
                self.idx += 1;
            }
            Ok(bits)
        } else {
            let bits_left_at_idx_plus_1 = bit_count - left_in_current_byte;
            let bits_left_at_idx = unsafe {
                *self.buf.get_unchecked(self.idx) << bits_left_at_idx_plus_1
            } & requested_mask;
            self.idx += 1;
            let bits_at_idx_plus_1 = unsafe {
                *self.buf.get_unchecked(self.idx) >> (8 - bits_left_at_idx_plus_1)
            };
            self.bit_idx = bits_left_at_idx_plus_1;
            Ok(bits_left_at_idx | bits_at_idx_plus_1)
        }
    }

    pub fn get_up_to_16(&mut self, bit_count: usize) -> Result<u16, Error> {
        if self.bits_left() < bit_count {
            return Err(Error::OutOfBounds);
        }
        if bit_count > 8 {
            let bits_hi = self.get_up_to_8(8)? as u16;
            let bits_lo = self.get_up_to_8(bit_count - 8)? as u16;
            Ok((bits_hi << (bit_count - 8)) | bits_lo)
        } else {
            Ok(self.get_up_to_8(bit_count)? as u16)
        }
    }


    pub fn des_bits<'di, T: DeserializeBits<'i>>(&'di mut self) -> Result<T, T::Error> {
        T::des_bits(self)
    }
}

/// Buffer writer that supports bit level operations
///
/// Use `bwr` as short name: let mut bwr = BitBufMut::new(..);
pub struct BitBufMut<'i> {
    pub(crate) buf: &'i mut [u8],
    // Maximum number of bits available (not whole slice might be available)
    pub(crate) len_bits: usize,
    // Next byte to write to
    pub(crate) idx: usize,
    // Next bit to write to, 0..=7
    pub(crate) bit_idx: usize,
}

impl<'i> BitBufMut<'i> {
    pub fn new(buf: &'i mut [u8], len_bits: usize) -> Result<Self, Error> {
        if len_bits >= buf.len() * 8 {
            return Err(Error::OutOfBounds);
        }
        Ok(BitBufMut {
            buf,
            len_bits,
            idx: 0,
            bit_idx: 0
        })
    }

    pub fn new_all(buf: &'i mut [u8]) -> Self {
        let len_bits = buf.len() * 8;
        BitBufMut {
            buf,
            len_bits,
            idx: 0,
            bit_idx: 0
        }
    }

    pub fn to_nibble_buf(self) -> Result<NibbleBufMut<'i>, Error> {
        if self.bit_idx != 0 && self.bit_idx != 4 {
            return Err(Error::UnalignedAccess);
        }
        Ok(NibbleBufMut {
            buf: self.buf,
            len_nibbles: self.len_bits / 4,
            idx: self.idx,
            is_at_byte_boundary: self.bit_idx == 0
        })
    }

    pub fn new_with_offset(buf: &'i mut [u8], offset_bits: usize, len_bits: usize) -> Result<Self, Error> {
        if (offset_bits + len_bits) > buf.len() * 8 || offset_bits > len_bits {
            return Err(Error::OutOfBounds);
        }
        Ok(BitBufMut {
            buf,
            len_bits: len_bits + offset_bits,
            idx: offset_bits / 8,
            bit_idx: offset_bits % 8,
        })
    }


    pub fn bit_pos(&self) -> usize {
        self.idx * 8 + self.bit_idx
    }

    pub fn bits_left(&self) -> usize {
        if !self.is_at_end() {
            self.len_bits - self.bit_pos()
        } else {
            0
        }
    }

    pub fn is_at_end(&self) -> bool {
        self.bit_pos() >= self.len_bits
    }

    pub fn is_at_nibble_boundary(&self) -> bool {
        self.bit_idx == 4
    }

    pub fn is_at_byte_boundary(&self) -> bool {
        self.bit_idx == 0
    }

    pub fn put_bit(&mut self, bit: bool) -> Result<(), Error> {
        let bit = if bit {
            1u8
        } else {
            0u8
        };
        self.put_up_to_8(1, bit)
    }

    pub fn put_up_to_8(&mut self, bit_count: usize, bits: u8) -> Result<(), Error> {
        if self.bits_left() < bit_count {
            return Err(Error::OutOfBounds);
        }

        let left_in_current_byte = 8 - self.bit_idx;
        let requested_mask = 0b1111_1111 >> (8 - bit_count);
        let bits = bits & requested_mask;
        if bit_count <= left_in_current_byte {
            unsafe {
                *self.buf.get_unchecked_mut(self.idx) |= bits << (left_in_current_byte - bit_count);
            };
            self.bit_idx += bit_count;
            if self.bit_idx >= 8 {
                self.bit_idx = 0;
                self.idx += 1;
            }
            Ok(())
        } else {
            let bits_to_idx_plus_1 = bit_count - left_in_current_byte;
            unsafe {
                *self.buf.get_unchecked_mut(self.idx) |= bits >> bits_to_idx_plus_1;
            };
            self.idx += 1;
            unsafe {
                *self.buf.get_unchecked_mut(self.idx) = bits << (8 - bits_to_idx_plus_1);
            };
            self.bit_idx = bits_to_idx_plus_1;
            Ok(())
        }
    }

    /// Put any type that implements SerializeBits into this buffer.
    pub fn put<E, T: SerializeBits<Error = E>>(&mut self, t: &T) -> Result<(), E> {
        t.ser_bits(self)
    }

    pub fn finish(self) -> (&'i mut [u8], usize, usize) {
        (self.buf, self.idx, self.bit_idx)
    }
}

#[cfg(test)]
mod test {
    use crate::serdes::bit_buf::BitBufMut;
    use super::{BitBuf, Error};

    #[test]
    fn get_up_to_8() {
        let buf = [0b1011_1100, 0b0101_1010];
        let mut rdr = BitBuf::new_all(&buf);
        assert_eq!(rdr.get_up_to_8(2), Ok(0b10));
        assert_eq!(rdr.get_up_to_8(8), Ok(0b1111_0001));
        assert_eq!(rdr.get_up_to_8(6), Ok(0b01_1010));
        assert_eq!(rdr.get_up_to_8(1), Err(Error::OutOfBounds));
    }

    #[test]
    fn out_of_bounds() {
        let buf = [0b1010_0000];
        let mut rdr = BitBuf::new(&buf, 5).unwrap();
        assert_eq!(rdr.get_up_to_8(5), Ok(0b1_0100));
        assert_eq!(rdr.get_up_to_8(1), Err(Error::OutOfBounds));
    }

    #[test]
    fn get_up_to_8_full_byte() {
        let buf = [0b1010_0101, 0b1111_0011];
        let mut rdr = BitBuf::new_all(&buf);
        assert_eq!(rdr.get_up_to_8(8), Ok(0b1010_0101));
        assert_eq!(rdr.get_up_to_8(8), Ok(0b1111_0011));
    }

    #[test]
    fn get_up_to_16() {
        let buf = [0b1010_1010, 0b0101_0101, 0b1100_0011];
        let mut rdr = BitBuf::new_all(&buf);
        assert_eq!(rdr.get_up_to_16(10), Ok(0b10_1010_1001));
        assert_eq!(rdr.get_up_to_16(14), Ok(0b01_0101_1100_0011));
    }

    #[test]
    fn get_bit() {
        let buf = [0b1010_0000];
        let mut rdr = BitBuf::new(&buf, 3).unwrap();
        assert_eq!(rdr.get_bit(), Ok(true));
        assert_eq!(rdr.get_bit(), Ok(false));
        assert_eq!(rdr.get_bit(), Ok(true));
        assert_eq!(rdr.get_bit(), Err(Error::OutOfBounds));
    }

    #[test]
    fn put_up_to_8() {
        let mut buf = [0u8; 2];
        let mut wgr = BitBufMut::new_all(&mut buf);
        assert_eq!(wgr.put_up_to_8(2, 0b10), Ok(()));
        assert_eq!(wgr.put_up_to_8(1, 0b0), Ok(()));
        assert_eq!(wgr.put_up_to_8(1, 0b1), Ok(()));
        assert_eq!(wgr.put_up_to_8(3, 0b111), Ok(()));
        assert_eq!(wgr.put_up_to_8(8, 0b01010101), Ok(()));
        assert_eq!(wgr.put_up_to_8(1, 0b0), Ok(()));
        let (buf, byte_pos, bit_pos) = wgr.finish();
        assert_eq!(buf[0], 0b1001_1110);
        assert_eq!(buf[1], 0b1010_1010);
        assert_eq!(byte_pos, 2);
        assert_eq!(bit_pos, 0);
    }

    #[test]
    fn put_bit() {
        let mut buf = [0u8; 1];
        let mut wgr = BitBufMut::new_all(&mut buf);
        wgr.put_bit(true).unwrap();
        wgr.put_bit(false).unwrap();
        wgr.put_bit(true).unwrap();
        wgr.put_bit(false).unwrap();
        wgr.put_bit(true).unwrap();
        let (buf, byte_pos, bit_pos) = wgr.finish();
        assert_eq!(buf[0], 0b10101_000);
        assert_eq!(byte_pos, 0);
        assert_eq!(bit_pos, 5);
    }

    #[test]
    fn to_nibble_buf() {
        let mut buf = [0u8; 2];
        let mut wgr = BitBufMut::new_all(&mut buf);
        wgr.put_bit(true).unwrap();
        wgr.put_up_to_8(3, 0b010).unwrap();

        let mut wgr = wgr.to_nibble_buf().unwrap();
        wgr.put_nibble(0b1100).unwrap();
        wgr.put_nibble(0b1010).unwrap();
        wgr.put_nibble(0b0011).unwrap();

        let (buf, pos, _) = wgr.finish();
        assert_eq!(buf[0], 0b1010_1100);
        assert_eq!(buf[1], 0b1010_0011);
        assert_eq!(pos, 2);
    }
}
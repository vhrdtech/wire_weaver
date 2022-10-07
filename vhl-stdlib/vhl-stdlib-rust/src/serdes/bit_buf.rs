// use thiserror::Error;
use crate::serdes::traits::SerializeBits;
use crate::serdes::{DeserializeBits, NibbleBufMut};

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
    /// Wrong bit count was supplied to put_up_to_*
    WrongBitCount,
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
            bit_idx: 0,
        })
    }

    pub fn new_all(buf: &'i [u8]) -> Self {
        BitBuf {
            buf,
            len_bits: buf.len() * 8,
            idx: 0,
            bit_idx: 0,
        }
    }

    pub fn new_with_offset(
        buf: &'i [u8],
        offset_bits: usize,
        len_bits: usize,
    ) -> Result<Self, Error> {
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
            let bits =
                unsafe { *self.buf.get_unchecked(self.idx) >> (left_in_current_byte - bit_count) }
                    & requested_mask;
            self.bit_idx += bit_count;
            if self.bit_idx >= 8 {
                self.bit_idx = 0;
                self.idx += 1;
            }
            Ok(bits)
        } else {
            let bits_left_at_idx_plus_1 = bit_count - left_in_current_byte;
            let bits_left_at_idx =
                unsafe { *self.buf.get_unchecked(self.idx) << bits_left_at_idx_plus_1 }
                    & requested_mask;
            self.idx += 1;
            let bits_at_idx_plus_1 =
                unsafe { *self.buf.get_unchecked(self.idx) >> (8 - bits_left_at_idx_plus_1) };
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
    /// Create a new bit writer covering len_bits from the provided array only.
    /// If less than len_bits is actually written, remaining portion of the buf will contain original bits.
    pub fn new(buf: &'i mut [u8], len_bits: usize) -> Result<Self, Error> {
        if len_bits >= buf.len() * 8 {
            return Err(Error::OutOfBounds);
        }
        Ok(BitBufMut {
            buf,
            len_bits,
            idx: 0,
            bit_idx: 0,
        })
    }

    /// Create a new bit writer covering whole provided array.
    /// If less than buf.len() * 8 is actually written, remaining portion of the buf will contain original bits.
    pub fn new_all(buf: &'i mut [u8]) -> Self {
        let len_bits = buf.len() * 8;
        BitBufMut {
            buf,
            len_bits,
            idx: 0,
            bit_idx: 0,
        }
    }

    /// Convert this bit writer into nibble writer.
    /// Only possible when bit position is 0 or 4 (i.e. at nibble bounds), otherwise error is returned.
    pub fn to_nibble_buf(self) -> Result<NibbleBufMut<'i>, Error> {
        if self.bit_idx != 0 && self.bit_idx != 4 {
            return Err(Error::UnalignedAccess);
        }
        Ok(NibbleBufMut {
            buf: self.buf,
            len_nibbles: self.len_bits / 4,
            idx: self.idx,
            is_at_byte_boundary: self.bit_idx == 0,
        })
    }

    /// Create bit writer starting at offset_bits and ending at offset_bits + len_bits in the provided array.
    pub fn new_with_offset(
        buf: &'i mut [u8],
        offset_bits: usize,
        len_bits: usize,
    ) -> Result<Self, Error> {
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
        let bit = if bit { 1u8 } else { 0u8 };
        self.put_up_to_8(1, bit)
    }

    /// Append 1 ..= 8 bits to the buffer.
    ///
    /// Internals:\
    /// □□□□□□□□ bit_idx = 0; left = 8\
    /// ■□□□□□□□ bit_idx = 1; left = 7\
    /// ■■□□□□□□ bit_idx = 2; left = 6\
    /// ■■■□□□□□ bit_idx = 3; left = 5\
    /// ■■■■□□□□ bit_idx = 4; left = 4\
    /// ■■■■■□□□ bit_idx = 5; left = 3\
    /// ■■■■■■□□ bit_idx = 6; left = 2\
    /// ■■■■■■■□ bit_idx = 7; left = 1
    ///
    /// if bit_count = 3 => requested_mask = □□□□□■■■\
    /// if bit_idx = 2; left = 6; then requested_mask << (left - bit_count) = □□■■■□□□\
    pub fn put_up_to_8(&mut self, bit_count: usize, bits: u8) -> Result<(), Error> {
        if self.bits_left() < bit_count {
            return Err(Error::OutOfBounds);
        }
        if bit_count == 0 {
            return Ok(());
        }
        if bit_count > 8 {
            return Err(Error::WrongBitCount);
        }

        let left_in_current_byte = 8 - self.bit_idx;
        let requested_mask = 0b1111_1111 >> (8 - bit_count);
        let bits = bits & requested_mask;
        if bit_count <= left_in_current_byte {
            unsafe {
                let b = self.buf.get_unchecked_mut(self.idx);
                *b &= !(requested_mask << (left_in_current_byte - bit_count));
                *b |= bits << (left_in_current_byte - bit_count);
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
                let b = self.buf.get_unchecked_mut(self.idx);
                *b &= !(requested_mask >> bits_to_idx_plus_1);
                *b |= bits >> bits_to_idx_plus_1;
            };
            self.idx += 1;
            unsafe {
                let b = self.buf.get_unchecked_mut(self.idx);
                *b &= !(requested_mask << (8 - bits_to_idx_plus_1));
                *b |= bits << (8 - bits_to_idx_plus_1);
            };
            self.bit_idx = bits_to_idx_plus_1;
            Ok(())
        }
    }

    pub fn put_up_to_16(&mut self, bit_count: usize, bits: u16) -> Result<(), Error> {
        if bit_count > 16 {
            return Err(Error::WrongBitCount);
        }
        if bit_count <= 8 {
            self.put_up_to_8(bit_count, (bits & 0xFF) as u8)?;
        } else {
            let msb = (bits >> 8) as u8;
            let lsb = (bits & 0xFF) as u8;
            self.put_up_to_8(bit_count - 8, msb)?;
            self.put_up_to_8(8, lsb)?;
        }
        Ok(())
    }

    /// Put any type that implements SerializeBits into this buffer.
    pub fn put<E, T: SerializeBits<Error=E>>(&mut self, t: &T) -> Result<(), E> {
        t.ser_bits(self)
    }

    pub fn finish(self) -> (&'i mut [u8], usize, usize) {
        (self.buf, self.idx, self.bit_idx)
    }
}

#[cfg(test)]
mod test {
    use super::{BitBuf, Error};
    use crate::serdes::bit_buf::BitBufMut;

    #[test]
    fn get_up_to_8() {
        let buf = [0b1011_1100, 0b0101_1010];
        let mut brd = BitBuf::new_all(&buf);
        assert_eq!(brd.get_up_to_8(2), Ok(0b10));
        assert_eq!(brd.get_up_to_8(8), Ok(0b1111_0001));
        assert_eq!(brd.get_up_to_8(6), Ok(0b01_1010));
        assert_eq!(brd.get_up_to_8(1), Err(Error::OutOfBounds));
    }

    #[test]
    fn out_of_bounds() {
        let buf = [0b1010_0000];
        let mut brd = BitBuf::new(&buf, 5).unwrap();
        assert_eq!(brd.get_up_to_8(5), Ok(0b1_0100));
        assert_eq!(brd.get_up_to_8(1), Err(Error::OutOfBounds));
    }

    #[test]
    fn get_up_to_8_full_byte() {
        let buf = [0b1010_0101, 0b1111_0011];
        let mut brd = BitBuf::new_all(&buf);
        assert_eq!(brd.get_up_to_8(8), Ok(0b1010_0101));
        assert_eq!(brd.get_up_to_8(8), Ok(0b1111_0011));
    }

    #[test]
    fn get_up_to_16() {
        let buf = [0b1010_1010, 0b0101_0101, 0b1100_0011];
        let mut brd = BitBuf::new_all(&buf);
        assert_eq!(brd.get_up_to_16(10), Ok(0b10_1010_1001));
        assert_eq!(brd.get_up_to_16(14), Ok(0b01_0101_1100_0011));
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
        let mut bwr = BitBufMut::new_all(&mut buf);
        assert_eq!(bwr.put_up_to_8(2, 0b10), Ok(()));
        assert_eq!(bwr.put_up_to_8(1, 0b0), Ok(()));
        assert_eq!(bwr.put_up_to_8(1, 0b1), Ok(()));
        assert_eq!(bwr.put_up_to_8(3, 0b111), Ok(()));
        assert_eq!(bwr.put_up_to_8(8, 0b01010101), Ok(()));
        assert_eq!(bwr.put_up_to_8(1, 0b0), Ok(()));
        let (buf, byte_pos, bit_pos) = bwr.finish();
        assert_eq!(buf[0], 0b1001_1110);
        assert_eq!(buf[1], 0b1010_1010);
        assert_eq!(byte_pos, 2);
        assert_eq!(bit_pos, 0);
    }

    #[test]
    fn put_bit() {
        let mut buf = [0u8; 1];
        let mut bwr = BitBufMut::new_all(&mut buf);
        bwr.put_bit(true).unwrap();
        bwr.put_bit(false).unwrap();
        bwr.put_bit(true).unwrap();
        bwr.put_bit(false).unwrap();
        bwr.put_bit(true).unwrap();
        let (buf, byte_pos, bit_pos) = bwr.finish();
        assert_eq!(buf[0], 0b10101_000);
        assert_eq!(byte_pos, 0);
        assert_eq!(bit_pos, 5);
    }

    #[test]
    fn to_nibble_buf() {
        let mut buf = [0u8; 2];
        let mut bwr = BitBufMut::new_all(&mut buf);
        bwr.put_bit(true).unwrap();
        bwr.put_up_to_8(3, 0b010).unwrap();

        let mut nwr = bwr.to_nibble_buf().unwrap();
        nwr.put_nibble(0b1100).unwrap();
        nwr.put_nibble(0b1010).unwrap();
        nwr.put_nibble(0b0011).unwrap();

        let (buf, pos, _) = nwr.finish();
        assert_eq!(buf[0], 0b1010_1100);
        assert_eq!(buf[1], 0b1010_0011);
        assert_eq!(pos, 2);
    }

    #[test]
    fn non_zero_buf() {
        let mut buf = [0xaa, 0xaa, 0xaa];
        let mut bwr = BitBufMut::new_all(&mut buf);
        bwr.put_up_to_8(2, 0b11).unwrap();
        bwr.put_up_to_8(5, 0b00000).unwrap();
        bwr.put_up_to_8(2, 0b00).unwrap();
        bwr.put_up_to_8(8, 0b11001100).unwrap();
        bwr.put_up_to_8(7, 0).unwrap();
        let (buf, _, _) = bwr.finish();
        assert_eq!(buf, [0b11000000, 0b01100110, 0b00000000]);
    }
}

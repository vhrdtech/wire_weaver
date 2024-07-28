use crate::nib16::Nib16;
use crate::{Error, SerializeShrinkWrap};

/// no_std buffer writer that supports 1 bit, 4 bit, variable length integer and other operations.
/// No alignment requirements are imposed on the byte buffer provided.
/// Allocator is not required for no_std use. See BufWriterOwned for std use.
///
/// # Example
/// ```
/// let mut buf = [0u8; 256];
/// let mut wr = shrink_wrap::BufWriter::new(&mut buf);
/// wr.write_bool(true).unwrap();
/// wr.write_u8(0xaa).unwrap();
/// let bytes = wr.finish().unwrap();
/// assert_eq!(bytes, &[0x80, 0xaa]);
/// ```
pub struct BufWriter<'i> {
    buf: &'i mut [u8],
    // Next byte to write to
    byte_idx: usize,
    // Next bit to write to
    bit_idx: u8,
    // Buffer length from the front, shrinks when write_u16_rev() is used.
    len_bytes: usize,
}

impl<'i> BufWriter<'i> {
    pub fn new(buf: &'i mut [u8]) -> Self {
        let len_bytes = buf.len();
        Self {
            buf,
            len_bytes,
            byte_idx: 0,
            bit_idx: 7,
        }
    }

    pub fn write_bool(&mut self, val: bool) -> Result<(), Error> {
        if (self.bytes_left() == 0) && self.bit_idx == 7 {
            return Err(Error::OutOfBounds);
        }
        self.buf[self.byte_idx] &= !(1 << self.bit_idx);
        self.buf[self.byte_idx] |= (val as u8) << self.bit_idx;
        if self.bit_idx == 0 {
            self.bit_idx = 7;
            self.byte_idx += 1;
        } else {
            self.bit_idx -= 1;
        }
        Ok(())
    }

    pub fn write_u4(&mut self, val: u8) -> Result<(), Error> {
        self.align_nibble();
        if (self.bytes_left() == 0) && self.bit_idx == 7 {
            return Err(Error::OutOfBounds);
        }
        if self.bit_idx == 7 {
            self.buf[self.byte_idx] &= 0b0000_1111;
            self.buf[self.byte_idx] |= val << 4;
            self.bit_idx = 3;
        } else {
            self.buf[self.byte_idx] &= 0b1111_0000;
            self.buf[self.byte_idx] |= val & 0b0000_1111;
            self.bit_idx = 7;
            self.byte_idx += 1;
        }
        Ok(())
    }

    pub fn write_u8(&mut self, val: u8) -> Result<(), Error> {
        self.align_byte();
        if self.bytes_left() == 0 {
            return Err(Error::OutOfBounds);
        }
        self.buf[self.byte_idx] = val;
        self.byte_idx += 1;
        Ok(())
    }

    pub fn write_u16(&mut self, val: u16) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())?;
        Ok(())
    }

    pub fn write_nib16(&mut self, val: u16) -> Result<(), Error> {
        Nib16(val).write_forward(self)
    }

    pub fn write_u16_rev(&mut self, val: u16) -> Result<U16RevPos, Error> {
        if self.bytes_left() < 2 {
            return Err(Error::OutOfBoundsRev);
        }
        let val_be = val.to_le_bytes();
        self.buf[self.len_bytes - 2] = val_be[0];
        self.buf[self.len_bytes - 1] = val_be[1];
        self.len_bytes -= 2;
        Ok(U16RevPos(self.len_bytes))
    }

    pub fn u16_rev_pos(&self) -> U16RevPos {
        U16RevPos(self.len_bytes)
    }

    pub fn update_u16_rev(&mut self, pos: U16RevPos, val: u16) -> Result<(), Error> {
        if pos.0 + 1 >= self.buf.len() {
            return Err(Error::OutOfBoundsRev);
        }
        let val_be = val.to_le_bytes();
        self.buf[pos.0] = val_be[0];
        self.buf[pos.0 + 1] = val_be[1];
        Ok(())
    }

    pub fn write_u32(&mut self, val: u32) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())?;
        Ok(())
    }

    pub fn write_u64(&mut self, val: u64) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())?;
        Ok(())
    }

    pub fn write_u128(&mut self, val: u128) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())?;
        Ok(())
    }

    pub fn write_i8(&mut self, val: i8) -> Result<(), Error> {
        self.write_u8(val as u8)
    }

    pub fn write_i16(&mut self, val: i16) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())?;
        Ok(())
    }

    pub fn write_i32(&mut self, val: i32) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())?;
        Ok(())
    }

    pub fn write_i64(&mut self, val: i64) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())?;
        Ok(())
    }

    pub fn write_i128(&mut self, val: i128) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())?;
        Ok(())
    }

    pub fn write_f32(&mut self, val: f32) -> Result<(), Error> {
        self.write_raw_slice(&val.to_bits().to_le_bytes())?;
        Ok(())
    }

    pub fn write_f64(&mut self, val: f64) -> Result<(), Error> {
        self.write_raw_slice(&val.to_bits().to_le_bytes())?;
        Ok(())
    }

    pub fn write_raw_slice(&mut self, val: &[u8]) -> Result<(), Error> {
        self.align_byte();
        if self.bytes_left() < val.len() {
            return Err(Error::OutOfBoundsRev);
        }
        self.buf[self.byte_idx..self.byte_idx + val.len()].copy_from_slice(val);
        self.byte_idx += val.len();
        Ok(())
    }

    pub fn write_bytes(&mut self, val: &[u8]) -> Result<(), Error> {
        let len = u16::try_from(val.len()).map_err(|_| Error::StrTooLong)?;
        self.write_u16_rev(len)?;
        self.write_raw_slice(val)
    }

    pub fn write_string(&mut self, val: &str) -> Result<(), Error> {
        let len = u16::try_from(val.len()).map_err(|_| Error::StrTooLong)?;
        self.write_u16_rev(len)?;
        self.write_raw_slice(val.as_bytes())
    }

    pub fn write<T: SerializeShrinkWrap>(&mut self, val: &T) -> Result<(), Error> {
        val.ser_shrink_wrap(self)
    }

    pub fn encode_nib16_rev(&mut self, from: U16RevPos, to: U16RevPos) -> Result<(), Error> {
        if to.0 < from.0 {
            return Ok(());
        }
        let reverse_u16_written = (to.0 - from.0) / 2;
        // dbg!(reverse_u16_written);
        if reverse_u16_written == 0 {
            return Ok(());
        }
        let mut total_nibbles = 0;
        let mut idx = from.0;
        for _ in 0..reverse_u16_written {
            let val = u16::from_le_bytes([self.buf[idx], self.buf[idx + 1]]);
            total_nibbles += Nib16(val).len_nibbles();
            idx += 2;
        }
        self.align_nibble();
        let not_at_byte_boundary = self.bit_idx != 7;
        if not_at_byte_boundary {
            total_nibbles += 1;
        }
        if total_nibbles % 2 != 0 {
            // ensure that reading from the back always starts from a valid Vlu16N
            self.write_u4(0).map_err(|_| Error::OutOfBoundsRevCompact)?;
        }

        let mut idx = self.len_bytes;
        for _ in 0..reverse_u16_written {
            let val = u16::from_le_bytes([self.buf[idx], self.buf[idx + 1]]);
            self.len_bytes += 2;
            Nib16(val).write_reversed(self)?;
            idx += 2;
        }
        debug_assert!(self.bit_idx == 7);
        Ok(())
    }

    pub fn finish(mut self) -> Result<&'i [u8], Error> {
        let reverse_u16_written = (self.buf.len() - self.len_bytes) / 2;
        self.encode_nib16_rev(U16RevPos(self.len_bytes), U16RevPos(self.buf.len()))?;
        if reverse_u16_written != 0 {
            Ok(&self.buf[0..self.byte_idx])
        } else {
            self.align_byte();
            Ok(&self.buf[0..self.byte_idx])
        }
    }

    fn align_nibble(&mut self) {
        if self.bit_idx == 7 || self.bit_idx == 3 {
            return;
        }
        if self.bit_idx > 3 {
            self.buf[self.byte_idx] &= !(0xFF >> (7 - self.bit_idx));
            self.bit_idx = 3;
        } else {
            self.bit_idx = 7;
            self.byte_idx += 1;
        }
    }

    pub fn align_byte(&mut self) {
        if self.bit_idx == 7 {
            return;
        }
        self.buf[self.byte_idx] &= !(0xFF >> (7 - self.bit_idx));
        self.bit_idx = 7;
        self.byte_idx += 1;
    }

    pub fn bytes_left(&mut self) -> usize {
        if self.byte_idx >= self.len_bytes {
            return 0;
        }
        if self.bit_idx == 7 {
            self.len_bytes - self.byte_idx
        } else {
            self.len_bytes - self.byte_idx - 1
        }
    }

    pub fn pos(&self) -> (usize, u8) {
        (self.byte_idx, self.bit_idx)
    }
}

#[derive(Debug, Copy, Clone)]
pub struct U16RevPos(usize);

#[cfg(test)]
mod tests {
    use crate::BufWriter;

    #[test]
    fn finish_zeroes_reserved_bits() {
        let mut buf = [0xFF; 64];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_bool(true).unwrap();
        wr.write_bool(false).unwrap();
        assert_eq!(wr.finish().unwrap(), &[0b1000_0000]);
    }

    #[test]
    fn write_u8_zeroes_reserved_bits() {
        let mut buf = [0xFF; 64];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_bool(true).unwrap();
        wr.write_bool(false).unwrap();
        wr.write_u8(0xAA).unwrap();
        assert_eq!(wr.finish().unwrap(), &[0b1000_0000, 0xAA]);
    }

    #[test]
    fn align_nibble_zeroes_reserved_bits() {
        let mut buf = [0xFF; 64];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_bool(true).unwrap();
        wr.write_bool(false).unwrap();
        wr.write_u4(0b1010).unwrap();
        assert_eq!(wr.finish().unwrap(), &[0b1000_1010]);
    }

    #[test]
    fn booleans() {
        let mut buf = [0xFF; 64];
        let mut wr = BufWriter::new(&mut buf);
        for b in [true, false, true, false, true, true, false, false] {
            wr.write_bool(b).unwrap();
        }
        assert_eq!(wr.bytes_left(), 63);
        assert_eq!(wr.finish().unwrap(), &[0b10101100]);
    }

    #[test]
    fn rev_u16_aligned() {
        let mut buf = [0; 6];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_u8(0xAA).unwrap();
        wr.write_u8(0xCC).unwrap();
        wr.write_u16_rev(3).unwrap();
        wr.write_u16_rev(5).unwrap();
        assert_eq!(wr.bytes_left(), 0);
        assert_eq!(&wr.buf, &[0xAA, 0xCC, 5, 0, 3, 0]);
        assert_eq!(wr.finish().unwrap(), &[0xAA, 0xCC, 0b0101_0011]);
    }

    #[test]
    fn rev_u16_unaligned() {
        let mut buf = [0; 9];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_u8(0xAA).unwrap();
        wr.write_u8(0xCC).unwrap();
        wr.write_u16_rev(3).unwrap();
        wr.write_u16_rev(5).unwrap();
        wr.write_u16_rev(7).unwrap();
        assert_eq!(wr.bytes_left(), 1);
        assert_eq!(&wr.buf, &[0xAA, 0xCC, 0, 7, 0, 5, 0, 3, 0]);
        assert_eq!(
            wr.finish().unwrap(),
            &[0xAA, 0xCC, 0b0000_0111, 0b0101_0011]
        );
    }

    #[test]
    fn rev_u16_smallest() {
        let mut buf = [0; 9];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_nib16(2).unwrap();
        wr.write_u16_rev(5).unwrap();
        assert_eq!(wr.finish().unwrap(), &[0x25]);
    }
}

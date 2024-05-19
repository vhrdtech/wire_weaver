use crate::vlu16n::Vlu16N;
use crate::Error::OutOfBoundsRev;
use crate::{DeserializeShrinkWrap, Error};

/// Buffer reader that treats input as a stream of nibbles.
pub struct BufReader<'i> {
    buf: &'i [u8],
    // Buffer length from the front, shrinks when read_vlu16n_rev() is used.
    len_bytes: usize,
    is_at_bit7_rev: bool,
    // Next byte to read from
    byte_idx: usize,
    // Next bit to read from
    bit_idx: u8,
}

impl<'i> BufReader<'i> {
    pub fn new(buf: &'i [u8]) -> Self {
        let len_bytes = buf.len();
        Self {
            buf,
            len_bytes,
            is_at_bit7_rev: false,
            byte_idx: 0,
            bit_idx: 7,
        }
    }

    pub fn read_bool(&mut self) -> Result<bool, Error> {
        if self.byte_idx >= self.len_bytes {
            return Err(Error::OutOfBounds);
        }
        let val = (self.buf[self.byte_idx] & (1 << self.bit_idx)) != 0;
        if self.bit_idx == 0 {
            self.bit_idx = 7;
            self.byte_idx += 1;
        } else {
            self.bit_idx -= 1;
        }
        Ok(val)
    }

    pub fn read_u4(&mut self) -> Result<u8, Error> {
        self.align_nibble();
        if self.byte_idx >= self.len_bytes {
            return Err(Error::OutOfBounds);
        }
        if self.bit_idx == 7 {
            self.bit_idx = 3;
            Ok(self.buf[self.byte_idx] >> 4)
        } else {
            let val = self.buf[self.byte_idx] & 0b1111;
            self.bit_idx = 7;
            self.byte_idx += 1;
            Ok(val)
        }
    }

    pub fn read_u8(&mut self) -> Result<u8, Error> {
        self.align_byte();
        if self.byte_idx >= self.len_bytes {
            return Err(Error::OutOfBounds);
        }
        let val = self.buf[self.byte_idx];
        self.byte_idx += 1;
        Ok(val)
    }

    pub fn read_u16(&mut self) -> Result<u16, Error> {
        let u16_bytes: [u8; 2] = self
            .read_slice(2)?
            .try_into()
            .map_err(|_| Error::OutOfBounds)?;
        Ok(u16::from_le_bytes(u16_bytes))
    }

    pub fn read_vlu16n(&mut self) -> Result<u16, Error> {
        Ok(Vlu16N::read_forward(self)?.0)
    }

    pub fn read_vlu16n_rev(&mut self) -> Result<u16, Error> {
        Ok(Vlu16N::read_reversed(self)?.0)
    }

    pub fn read_u32(&mut self) -> Result<u32, Error> {
        let u32_bytes: [u8; 4] = self
            .read_slice(4)?
            .try_into()
            .map_err(|_| Error::OutOfBounds)?;
        Ok(u32::from_le_bytes(u32_bytes))
    }

    pub fn read_u64(&mut self) -> Result<u64, Error> {
        let u64_bytes: [u8; 8] = self
            .read_slice(8)?
            .try_into()
            .map_err(|_| Error::OutOfBounds)?;
        Ok(u64::from_le_bytes(u64_bytes))
    }

    pub fn read_u128(&mut self) -> Result<u128, Error> {
        let u128_bytes: [u8; 16] = self
            .read_slice(16)?
            .try_into()
            .map_err(|_| Error::OutOfBounds)?;
        Ok(u128::from_le_bytes(u128_bytes))
    }

    pub fn read_i8(&mut self) -> Result<i8, Error> {
        Ok(self.read_u8()? as i8)
    }

    pub fn read_i16(&mut self) -> Result<i16, Error> {
        let i16_bytes: [u8; 2] = self
            .read_slice(2)?
            .try_into()
            .map_err(|_| Error::OutOfBounds)?;
        Ok(i16::from_le_bytes(i16_bytes))
    }

    pub fn read_i32(&mut self) -> Result<i32, Error> {
        let i32_bytes: [u8; 4] = self
            .read_slice(4)?
            .try_into()
            .map_err(|_| Error::OutOfBounds)?;
        Ok(i32::from_le_bytes(i32_bytes))
    }

    pub fn read_i64(&mut self) -> Result<i64, Error> {
        let i64_bytes: [u8; 8] = self
            .read_slice(8)?
            .try_into()
            .map_err(|_| Error::OutOfBounds)?;
        Ok(i64::from_le_bytes(i64_bytes))
    }

    pub fn read_i128(&mut self) -> Result<i128, Error> {
        let i128_bytes: [u8; 16] = self
            .read_slice(16)?
            .try_into()
            .map_err(|_| Error::OutOfBounds)?;
        Ok(i128::from_le_bytes(i128_bytes))
    }

    pub fn read_f32(&mut self) -> Result<f32, Error> {
        let f32_bytes: [u8; 4] = self
            .read_slice(4)?
            .try_into()
            .map_err(|_| Error::OutOfBounds)?;
        Ok(f32::from_le_bytes(f32_bytes))
    }

    pub fn read_f64(&mut self) -> Result<f64, Error> {
        let f64_bytes: [u8; 8] = self
            .read_slice(8)?
            .try_into()
            .map_err(|_| Error::OutOfBounds)?;
        Ok(f64::from_le_bytes(f64_bytes))
    }

    pub fn read_slice(&mut self, len: usize) -> Result<&'i [u8], Error> {
        self.align_byte();
        if self.byte_idx + len > self.len_bytes {
            return Err(Error::OutOfBounds);
        }
        let val = &self.buf[self.byte_idx..self.byte_idx + len];
        self.byte_idx += len;
        Ok(val)
    }

    pub fn read_str(&mut self) -> Result<&'i str, Error> {
        let len_bytes = self.read_vlu16n_rev()? as usize;
        let str_bytes = self.read_slice(len_bytes)?;
        core::str::from_utf8(str_bytes).map_err(|_| Error::MalformedUtf8)
    }

    pub fn read<T: DeserializeShrinkWrap<'i>>(&mut self) -> Result<T, Error> {
        T::des_shrink_wrap(self)
    }

    pub fn split(&mut self, len: usize) -> Result<Self, Error> {
        self.align_byte();
        if self.byte_idx + len > self.len_bytes {
            return Err(Error::OutOfBounds);
        }
        let prev_byte_idx = self.byte_idx;
        self.byte_idx += len;
        Ok(BufReader {
            buf: &self.buf[prev_byte_idx..prev_byte_idx + len],
            len_bytes: len,
            is_at_bit7_rev: false,
            byte_idx: 0,
            bit_idx: 7,
        })
    }

    pub(crate) fn read_u4_rev(&mut self) -> Result<u8, Error> {
        if self.byte_idx >= self.len_bytes {
            return Err(OutOfBoundsRev);
        }
        if self.is_at_bit7_rev {
            self.is_at_bit7_rev = false;
            self.len_bytes -= 1;
            Ok(self.buf[self.len_bytes] >> 4)
        } else {
            self.is_at_bit7_rev = true;
            Ok(self.buf[self.len_bytes - 1] & 0b1111)
        }
    }

    fn align_nibble(&mut self) {
        if self.bit_idx == 7 || self.bit_idx == 3 {
            return;
        }
        if self.bit_idx > 3 {
            self.bit_idx = 3;
        } else {
            self.bit_idx = 7;
            self.byte_idx += 1;
        }
    }

    fn align_byte(&mut self) {
        if self.bit_idx == 7 {
            return;
        }
        self.bit_idx = 7;
        self.byte_idx += 1;
    }

    pub fn bytes_left(&mut self) -> usize {
        if self.byte_idx <= self.len_bytes {
            let rev_read = if self.is_at_bit7_rev { 1 } else { 0 };
            if self.bit_idx == 7 {
                self.len_bytes - self.byte_idx - rev_read
            } else if self.byte_idx < self.len_bytes {
                self.len_bytes - self.byte_idx - 1 - rev_read
            } else {
                0
            }
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::BufReader;

    #[test]
    fn bytes() {
        let buf = [1, 2, 3];
        let mut rd = BufReader::new(&buf);
        assert_eq!(rd.read_u8(), Ok(1));
        assert_eq!(rd.bytes_left(), 2);
        assert_eq!(rd.read_u8(), Ok(2));
        assert_eq!(rd.bytes_left(), 1);
        assert_eq!(rd.read_u8(), Ok(3));
        assert_eq!(rd.bytes_left(), 0);
    }

    #[test]
    fn float() {
        let buf = [0x3E, 0x80, 0, 0];
        let mut rd = BufReader::new(&buf);
        assert_eq!(rd.read_f32(), Ok(0.25));
        assert_eq!(rd.bytes_left(), 0);
    }

    #[test]
    fn rev_read_bytes_left() {
        let buf = [0x35];
        let mut rd = BufReader::new(&buf);
        assert_eq!(rd.bytes_left(), 1);
        let _ = rd.read_vlu16n_rev().unwrap();
        assert_eq!(rd.bytes_left(), 0);
        let _ = rd.read_vlu16n_rev().unwrap();
        assert_eq!(rd.bytes_left(), 0);
    }
}

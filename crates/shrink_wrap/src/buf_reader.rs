use crate::vlu16n::Vlu16N;
use crate::Error;
use crate::Error::OutOfBoundsRev;

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

    pub fn read_f32(&mut self) -> Result<f32, Error> {
        let f32_bytes: [u8; 4] = self
            .read_slice(4)?
            .try_into()
            .map_err(|_| Error::OutOfBounds)?;
        Ok(f32::from_be_bytes(f32_bytes))
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

    pub fn read_vlu16n(&mut self) -> Result<u16, Error> {
        Ok(Vlu16N::read_forward(self)?.0)
    }

    pub fn read_vlu16n_rev(&mut self) -> Result<u16, Error> {
        Ok(Vlu16N::read_reversed(self)?.0)
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

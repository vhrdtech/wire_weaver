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
}

use crate::nib32::UNib32;
use crate::un::write_unx;
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

    /// Write on bit to the buffer. One can write 8 bits with this function and only one byte will be used in the buffer.
    /// Nibble writes will align the buffer to nibble boundary and byte writes to byte boundary.
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

    /// Write one nibble, 4 lower bits are used and higher bits are ignored.
    pub fn write_u4(&mut self, val: u8) -> Result<(), Error> {
        self.align_nibble();
        if self.nibbles_left() == 0 {
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

    write_unx!(write_un8, u8, 8);
    write_unx!(write_un16, u16, 16);
    write_unx!(write_un32, u32, 32);
    write_unx!(write_un64, u64, 64);

    /// Write u8.
    pub fn write_u8(&mut self, val: u8) -> Result<(), Error> {
        self.align_byte();
        if self.bytes_left() == 0 {
            return Err(Error::OutOfBounds);
        }
        self.buf[self.byte_idx] = val;
        self.byte_idx += 1;
        Ok(())
    }

    /// Write u16 in Little Endian.
    pub fn write_u16(&mut self, val: u16) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())?;
        Ok(())
    }

    /// Write u16 in Nib16 forward encoding. It will take from 1 nibble to 2 bytes in the buffer,
    /// depending on the number.
    pub fn write_unib32(&mut self, val: u32) -> Result<(), Error> {
        UNib32(val).write_forward(self)
    }

    /// Write u16 to the back of the buffer, later when [BufWriter::encode_nib16_rev()] or [BufWriter::finish()]
    /// are called, all the numbers will be encoded to Nib16 reverse encoding.
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

    /// See [BufWriter::encode_nib16_rev()] on how this function is used.
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

    /// Write u32 in Little Endian.
    pub fn write_u32(&mut self, val: u32) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())?;
        Ok(())
    }

    /// Write u64 in Little Endian.
    pub fn write_u64(&mut self, val: u64) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())?;
        Ok(())
    }

    /// Write u128 in Little Endian.
    pub fn write_u128(&mut self, val: u128) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())?;
        Ok(())
    }

    /// Write i8.
    pub fn write_i8(&mut self, val: i8) -> Result<(), Error> {
        self.write_u8(val as u8)
    }

    /// Write i16 in Little Endian.
    pub fn write_i16(&mut self, val: i16) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())?;
        Ok(())
    }

    /// Write i32 in Little Endian.
    pub fn write_i32(&mut self, val: i32) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())?;
        Ok(())
    }

    /// Write i64 in Little Endian.
    pub fn write_i64(&mut self, val: i64) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())?;
        Ok(())
    }

    /// Write i128 in Little Endian.
    pub fn write_i128(&mut self, val: i128) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())?;
        Ok(())
    }

    /// Write f32 in Little Endian.
    pub fn write_f32(&mut self, val: f32) -> Result<(), Error> {
        self.write_raw_slice(&val.to_bits().to_le_bytes())?;
        Ok(())
    }

    /// Write f64 in Little Endian.
    pub fn write_f64(&mut self, val: f64) -> Result<(), Error> {
        self.write_raw_slice(&val.to_bits().to_le_bytes())?;
        Ok(())
    }

    /// Write the provided slice to the buffer as is. Note that you won't be able to read it back
    /// with BufReader without knowing the length, which is not written in this case.
    /// See [BufWriter::write_bytes()] for variable length slices.
    pub fn write_raw_slice(&mut self, val: &[u8]) -> Result<(), Error> {
        self.align_byte();
        if self.bytes_left() < val.len() {
            return Err(Error::OutOfBoundsRev);
        }
        self.buf[self.byte_idx..self.byte_idx + val.len()].copy_from_slice(val);
        self.byte_idx += val.len();
        Ok(())
    }

    /// Write variable length slice, with length written to the back of the buffer.
    pub fn write_bytes(&mut self, val: &[u8]) -> Result<(), Error> {
        let len = u16::try_from(val.len()).map_err(|_| Error::StrTooLong)?;
        self.write_u16_rev(len)?;
        self.write_raw_slice(val)
    }

    pub fn fill_nibbles(&mut self, val: u8) {
        if self.write_u4(val).is_err() {
            return;
        }
        let val = val & 0b0000_1111;
        let val = val | (val << 4);
        self.fill_bytes(val);
    }

    pub fn fill_bytes(&mut self, val: u8) {
        let bytes_left = self.bytes_left();
        if bytes_left == 0 {
            return;
        }
        self.buf[self.byte_idx..].fill(val);
    }

    /// Write variable length string, with length written to the back of the buffer.
    pub fn write_string(&mut self, val: &str) -> Result<(), Error> {
        let len = u16::try_from(val.len()).map_err(|_| Error::StrTooLong)?;
        self.write_u16_rev(len)?;
        self.write_raw_slice(val.as_bytes())
    }

    /// Write any object that implements SerializeShrinkWrap trait.
    pub fn write<T: SerializeShrinkWrap>(&mut self, val: &T) -> Result<(), Error> {
        val.ser_shrink_wrap(self)
    }

    /// Encode some of the numbers previously written to the back of the buffer (for example
    /// when writing variable length slices, strings or objects).
    /// This operations allows to preserve backwards and forwards compatibility:
    /// * newer data read by old code: additional bytes can be ignored.
    /// * old data read by new code: missing bytes are expected and None or 0 length arrays are created.
    ///
    /// This function is primarily intended to be used in wire_weaver auto generated code.
    /// Example of how this function is used in wire_weaver when serializing variable length object:
    /// ```
    /// use shrink_wrap::BufWriter;
    /// let mut buf = [0u8; 128];
    /// let mut wr = BufWriter::new(&mut buf);
    ///
    /// let size_slot_pos = wr.write_u16_rev(0).unwrap(); // reserve u16_rev slot in the back of the buffer
    /// let unsized_start_bytes = wr.pos().0; // remember current position in bytes
    /// // Write object of unknown size, potentially containing more objects with variable length,
    /// // which in turn will write more u16_rev numbers to the back of the buffer.
    /// let unsized_object = vec![1u8, 2, 3];
    /// wr.write(&unsized_object).unwrap();
    /// // Encode u16_rev numbers written by the object itself to Nib16 reverse encoding, if any
    /// wr.encode_nib16_rev(wr.u16_rev_pos(), size_slot_pos).unwrap();
    /// wr.align_byte(); // Variable sized objects must be byte aligned, because length is in bytes and to not shift the whole buffer by less than one byte
    /// // Calculate the size of the variable length object + all of the u16_rev numbers it might have used in Nib16 reverse encoding.
    /// let size_bytes = wr.pos().0 - unsized_start_bytes;
    /// let size_bytes = u16::try_from(size_bytes).unwrap();
    /// assert_eq!(size_bytes, 4);
    /// // Update the original slot with an actual size.
    /// wr.update_u16_rev(size_slot_pos, size_bytes).unwrap();
    /// let buf = wr.finish().unwrap();
    /// assert_eq!(buf, &[1, 2, 3, 3, 4]);
    /// println!("{buf:02x?}");
    ///```
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
            total_nibbles += UNib32(val as u32).len_nibbles();
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
            UNib32(val as u32).write_reversed(self)?;
            idx += 2;
        }
        debug_assert!(self.bit_idx == 7);
        Ok(())
    }

    /// Align to byte, encode all the remaining numbers written to the back of the buffer, align to byte and
    /// return the slice containing written data.
    pub fn finish(&mut self) -> Result<&[u8], Error> {
        // self.align_byte();
        let reverse_u16_written = (self.buf.len() - self.len_bytes) / 2;
        if reverse_u16_written > 0 {
            self.encode_nib16_rev(U16RevPos(self.len_bytes), U16RevPos(self.buf.len()))?;
        } else {
            self.align_byte();
        }
        let byte_idx = self.byte_idx;
        self.byte_idx = 0;
        self.bit_idx = 7;
        self.len_bytes = self.buf.len();
        Ok(&self.buf[0..byte_idx])
    }

    /// Align to byte, encode all the remaining numbers written to the back of the buffer, align to byte and
    /// return the slice containing written data.
    ///
    /// This method takes self by value, allowing one to return the slice from functions.
    pub fn finish_and_take(mut self) -> Result<&'i [u8], Error> {
        let len = self.finish()?.len();
        Ok(&self.buf[0..len])
    }

    /// Simply return the buffer, note that buffer is not set to zero and might contain old data.
    pub fn deinit(self) -> &'i mut [u8] {
        self.buf
    }

    /// Align writer to the next nibble if not already, setting remaining bits to zero.
    #[inline]
    pub fn align_nibble(&mut self) {
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

    /// Align writer to the next byte if not already, setting remaining bits to zero.
    #[inline]
    pub fn align_byte(&mut self) {
        if self.bit_idx == 7 {
            return;
        }
        self.buf[self.byte_idx] &= !(0xFF >> (7 - self.bit_idx));
        self.bit_idx = 7;
        self.byte_idx += 1;
    }

    /// Return the number of bytes left.
    /// Note that there might be space for some bits or a nibble when this function returns 0.
    #[inline]
    pub fn bytes_left(&self) -> usize {
        if self.byte_idx >= self.len_bytes {
            return 0;
        }
        if self.bit_idx == 7 {
            self.len_bytes - self.byte_idx
        } else {
            self.len_bytes - self.byte_idx - 1
        }
    }

    /// Returns the number of nibbles left. Note that there might be space for 0 to 3 bits when this function returns 0.
    #[inline]
    pub fn nibbles_left(&self) -> usize {
        self.bytes_left() * 2 + if self.bit_idx == 3 { 1 } else { 0 }
    }

    /// Return the current position in bytes and bits.
    #[inline]
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
        wr.write_unib32(2).unwrap();
        wr.write_u16_rev(5).unwrap();
        assert_eq!(wr.finish().unwrap(), &[0x25]);
    }

    #[test]
    fn align_on_finish() {
        let mut buf = [0u8; 64];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_bool(true).unwrap();
        let buf = wr.finish().unwrap();
        assert_eq!(buf, &[0b1000_0000]);
    }

    #[test]
    fn write_un() {
        let mut buf = [0; 64];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_bool(true).unwrap();
        wr.write_un8(7, 0b010_1010).unwrap();
        wr.write_un8(3, 0b110).unwrap();
        wr.write_un16(12, 0b1011_1001_0100).unwrap();
        wr.write_un32(17, 0b1_10101111_01010011).unwrap();
        let buf = wr.finish().unwrap();
        assert_eq!(
            buf,
            &[
                0b1010_1010,
                0b1101_0111,
                0b0010_1001,
                0b10101111,
                0b01010011
            ]
        );
    }
}

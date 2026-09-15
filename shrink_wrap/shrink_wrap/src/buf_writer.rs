use crate::nib32::UNib32;
use crate::un::write_unx;
use crate::{ElementSize, Error, Nibble, SerializeShrinkWrap};

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
    /// Next byte to write to
    byte_idx: usize,
    /// Next bit to write to, starts from 7
    bit_idx: u8,
    /// Buffer length from the front, shrinks when [Self::write_rev_len()] is used.
    len_bytes: usize,
}

/// Buffer writer state that can be used to jump back and fill in some data.
#[derive(Copy, Clone)]
pub struct BufWriterState {
    byte_idx: usize,
    bit_idx: u8,
    len_bytes: usize,
}

impl BufWriterState {
    /// Position (byte index, bit index) at the time of [BufWriter::save_state].
    pub fn pos(&self) -> (usize, u8) {
        (self.byte_idx, self.bit_idx)
    }
}

/// Builder-style serialization token for 'Unsized' types.
pub struct UnsizedBuilder {
    size_slot_pos: RevPos,
    unsized_start_idx: usize,
}

impl<'i> BufWriter<'i> {
    /// Create a new BufWriter from the provided mutable slice.
    /// `buf` does not need to be initialized to zero.
    pub fn new(buf: &'i mut [u8]) -> Self {
        let len_bytes = buf.len();
        Self {
            buf,
            len_bytes,
            byte_idx: 0,
            bit_idx: 7,
        }
    }

    /// Reset BufWriter to the beginning, "forgetting" all written data.
    /// Usefull when re-using the same writer multiple times.
    pub fn reset(&mut self) {
        self.len_bytes = self.buf.len();
        self.byte_idx = 0;
        self.bit_idx = 7;
    }

    /// Write on bit to the buffer. One can write 8 bits with this function, and only one byte will be used in the buffer.
    /// Nibble writes will align the buffer to nibble boundary and byte writes to byte boundary.
    pub fn write_bool(&mut self, val: bool) -> Result<(), Error> {
        if (self.bytes_left() == 0) && self.bit_idx == 7 {
            return Err(Error::OutOfBoundsWriteBool);
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

    /// Align to nibble and write one [Nibble], 4 lower bits are used, and higher bits are ignored.
    /// Note that there is a method write_un8(4, val), that will also write 4 bits to the buffer,
    /// but will use an alignment of 1 bit instead.
    pub fn write_nib(&mut self, val: Nibble) -> Result<(), Error> {
        self.align_nibble();
        if self.nibbles_left() == 0 {
            return Err(Error::OutOfBoundsWriteU4);
        }
        if self.bit_idx == 7 {
            self.buf[self.byte_idx] &= 0b0000_1111;
            self.buf[self.byte_idx] |= val.value() << 4;
            self.bit_idx = 3;
        } else {
            self.buf[self.byte_idx] &= 0b1111_0000;
            self.buf[self.byte_idx] |= val.value();
            self.bit_idx = 7;
            self.byte_idx += 1;
        }
        Ok(())
    }

    /// Align to nibble and write one [Nibble], 4 lower bits are used, and higher bits are ignored.
    /// See also [write_nib](Self::write_nib)
    pub fn write_nib_masked(&mut self, val: u8) -> Result<(), Error> {
        self.write_nib(Nibble::new_masked(val))
    }

    write_unx!(write_un8, u8, 8);
    write_unx!(write_un16, u16, 16);
    write_unx!(write_un32, u32, 32);
    write_unx!(write_un64, u64, 64);

    /// Write u8 with alignment of 1 byte.
    pub fn write_u8(&mut self, val: u8) -> Result<(), Error> {
        self.align_byte();
        if self.bytes_left() == 0 {
            return Err(Error::OutOfBoundsWriteU8);
        }
        self.buf[self.byte_idx] = val;
        self.byte_idx += 1;
        Ok(())
    }

    /// Write u16 in Little Endian and alignment of 1 byte.
    pub fn write_u16(&mut self, val: u16) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())?;
        Ok(())
    }

    /// Write u16 in UNib32 forward encoding. It will take from 1 nibble to 11 nibbles in the buffer,
    /// depending on the number. Alignment of 4 bits is used.
    pub fn write_unib32(&mut self, val: u32) -> Result<(), Error> {
        UNib32(val).write_forward(self)
    }

    /// Write len to the back of the buffer, later when [BufWriter::encode_len_fifo()] or [BufWriter::finish()]
    /// are called, all the numbers will be encoded to UNib32 reverse encoding.
    ///
    /// NOTE: currently BufWriter uses u16 numbers, so maximum length of an object is 65_535.
    /// BufWriterOwned uses u32, so this limit does not apply there.
    /// This might be changed in the future, but for now 65K on no alloc seems acceptable.
    pub fn write_rev_len(&mut self, len: usize) -> Result<RevPos, Error> {
        let Ok(len) = u16::try_from(len) else {
            return Err(Error::LenTooLong);
        };
        if self.bytes_left() < 2 {
            return Err(Error::OutOfBoundsRev);
        }
        let val_be = len.to_le_bytes();
        self.buf[self.len_bytes - 2] = val_be[0];
        self.buf[self.len_bytes - 1] = val_be[1];
        self.len_bytes -= 2;
        #[cfg(feature = "tracing-extended")]
        tracing::trace!("written u16 rev = {val} at pos = {}", self.len_bytes);
        Ok(RevPos(self.len_bytes))
    }

    /// See [BufWriter::encode_len_fifo()] on how this function is used.
    pub fn rev_len_pos(&self) -> RevPos {
        RevPos(self.len_bytes)
    }

    /// Update previously written u16 value in the back of the buffer, using the obtained index.
    pub fn update_rev_len(&mut self, pos: RevPos, len: usize) -> Result<(), Error> {
        if pos.0 + 1 >= self.buf.len() {
            return Err(Error::OutOfBoundsRev);
        }
        let len = u16::try_from(len).map_err(|_| Error::LenTooLong)?;
        let val_be = len.to_le_bytes();
        self.buf[pos.0] = val_be[0];
        self.buf[pos.0 + 1] = val_be[1];
        #[cfg(feature = "tracing-extended")]
        tracing::trace!("updated u16 rev at pos{} = {val}", pos.0);
        Ok(())
    }

    /// Write u32 in Little Endian and alignment of 1 byte.
    pub fn write_u32(&mut self, val: u32) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())?;
        Ok(())
    }

    /// Write u64 in Little Endian and alignment of 1 byte.
    pub fn write_u64(&mut self, val: u64) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())?;
        Ok(())
    }

    /// Write u128 in Little Endian and alignment of 1 byte.
    pub fn write_u128(&mut self, val: u128) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())?;
        Ok(())
    }

    /// Write i8 and alignment of 1 byte.
    pub fn write_i8(&mut self, val: i8) -> Result<(), Error> {
        self.write_u8(val as u8)
    }

    /// Write i16 in Little Endian and alignment of 1 byte.
    pub fn write_i16(&mut self, val: i16) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())?;
        Ok(())
    }

    /// Write i32 in Little Endian and alignment of 1 byte.
    pub fn write_i32(&mut self, val: i32) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())?;
        Ok(())
    }

    /// Write i64 in Little Endian and alignment of 1 byte.
    pub fn write_i64(&mut self, val: i64) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())?;
        Ok(())
    }

    /// Write i128 in Little Endian and alignment of 1 byte.
    pub fn write_i128(&mut self, val: i128) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())?;
        Ok(())
    }

    /// Write f32 in Little Endian and alignment of 1 byte.
    pub fn write_f32(&mut self, val: f32) -> Result<(), Error> {
        self.write_raw_slice(&val.to_bits().to_le_bytes())?;
        Ok(())
    }

    /// Write f64 in Little Endian and alignment of 1 byte.
    pub fn write_f64(&mut self, val: f64) -> Result<(), Error> {
        self.write_raw_slice(&val.to_bits().to_le_bytes())?;
        Ok(())
    }

    /// Write the provided slice to the buffer as is. Note that you won't be able to read it back
    /// with BufReader without knowing the length, which is not written in this case.
    /// See also [Self::write_bytes].
    pub fn write_raw_slice(&mut self, val: &[u8]) -> Result<(), Error> {
        self.align_byte();
        if self.bytes_left() < val.len() {
            return Err(Error::OutOfBoundsWriteRawSlice);
        }
        self.buf[self.byte_idx..self.byte_idx + val.len()].copy_from_slice(val);
        self.byte_idx += val.len();
        Ok(())
    }

    // Write variable length slice, with length written to the back of the buffer.
    // pub fn write_bytes(&mut self, val: &[u8]) -> Result<(), Error> {
    //     let len = u16::try_from(val.len()).map_err(|_| Error::StrTooLong)?;
    //     self.write_rev_len(len)?;
    //     self.write_raw_slice(val)
    // }

    pub fn fill_nibbles(&mut self, val: u8) {
        if self.write_nib(Nibble::new_masked(val)).is_err() {
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

    /// Write variable length slice to the buffer. Length will be written to the back.
    pub fn write_bytes(&mut self, val: &[u8]) -> Result<(), Error> {
        self.write_rev_len(val.len())?;
        self.write_raw_slice(val)
    }

    /// Write variable length string to the buffer. Length will be written to the back.
    pub fn write_str(&mut self, val: &str) -> Result<(), Error> {
        self.write_rev_len(val.len())?;
        self.write_raw_slice(val.as_bytes())
    }

    /// Write any value that implements SerializeShrinkWrap trait.
    ///
    /// If the value is Unsized, then size is calculated and written to the back of the buffer as u16.
    /// Which is later encoded to reverse UNib32.
    ///
    /// Note that for serializing root structs or enums, it's better to call ser_shrink_wrap directly,
    /// as it avoids wasting space for the object size, which is known from the buffer size itself.
    ///
    /// Values serialized with this method must be deserialized with [read](crate::BufReader::read).
    /// Values serialized with [ser_shrink_wrap](SerializeShrinkWrap::ser_shrink_wrap) must be
    /// deserialized with [des_shrink_wrap](crate::DeserializeShrinkWrap::des_shrink_wrap).
    /// (because of the additional size written by write and expected by the read).
    pub fn write<T: SerializeShrinkWrap>(&mut self, val: &T) -> Result<(), Error> {
        let unsized_builder = if matches!(T::ELEMENT_SIZE, ElementSize::Unsized) {
            Some(UnsizedBuilder::new(self)?)
        } else {
            None
        };
        val.ser_shrink_wrap(self)?;
        if let Some(builder) = unsized_builder {
            builder.finish(self)?;
        }
        Ok(())
    }

    /// Encode some of the numbers previously written to the back of the buffer (for example,
    /// when writing variable length slices, strings or objects).
    /// This operation allows preserving backwards and forwards compatibility:
    /// * newer data read by old code: additional bytes can be ignored.
    /// * old data read by new code: missing bytes are expected and None or 0 length arrays are created.
    ///
    /// This function is primarily intended to be used in wire_weaver auto generated code.
    /// Example of how this function is used in wire_weaver when serializing a variable length object:
    /// ```
    /// use shrink_wrap::BufWriter;
    /// let mut buf = [0u8; 128];
    /// let mut wr = BufWriter::new(&mut buf);
    ///
    /// let size_slot_pos = wr.write_rev_len(0).unwrap(); // reserve u16_rev slot in the back of the buffer
    /// let unsized_start_bytes = wr.pos().0; // remember current position in bytes
    /// // Write an object of unknown size, potentially containing more objects with variable length,
    /// // which in turn will write more u16_rev numbers to the back of the buffer.
    /// let unsized_object = vec![1u8, 2, 3];
    /// wr.write(&unsized_object).unwrap();
    /// // Encode u16_rev numbers written by the object itself to UNib32 reverse encoding, if any
    /// wr.encode_len_fifo(wr.rev_len_pos(), size_slot_pos).unwrap();
    /// wr.align_byte(); // Variable sized objects must be byte aligned, because length is in bytes and to not shift the whole buffer by less than one byte
    /// // Calculate the size of the variable length object + all the u16_rev numbers it might have used in Nib16 reverse encoding.
    /// let size_bytes = wr.pos().0 - unsized_start_bytes;
    /// let size_bytes = u16::try_from(size_bytes).unwrap();
    /// assert_eq!(size_bytes, 4);
    /// // Update the original slot with an actual size.
    /// wr.update_rev_len(size_slot_pos, size_bytes).unwrap();
    /// let buf = wr.finish().unwrap();
    /// assert_eq!(buf, &[1, 2, 3, 3, 4]);
    /// println!("{buf:02x?}");
    ///```
    pub fn encode_len_fifo(&mut self, from: RevPos, to: RevPos) -> Result<(), Error> {
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
            self.write_nib(Nibble::zero())
                .map_err(|_| Error::OutOfBoundsRevCompact)?;
        }

        let mut idx = self.len_bytes;
        for _ in 0..reverse_u16_written {
            let val = u16::from_le_bytes([self.buf[idx], self.buf[idx + 1]]);
            self.len_bytes += 2;
            UNib32(val as u32).write_reversed(self)?;
            #[cfg(feature = "tracing-extended")]
            tracing::trace!("encoded rev.UNib32 = {val}");
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
            self.encode_len_fifo(RevPos(self.len_bytes), RevPos(self.buf.len()))?;
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

    /// Return the buffer, note that buffer is not set to zero and might contain old data.
    pub fn deinit(self) -> &'i mut [u8] {
        self.buf
    }

    pub fn buf(&'i self) -> &'i [u8] {
        self.buf
    }

    /// Align writer to the next nibble if not already, setting the remaining bits to zero.
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

    /// Align writer to the next byte if not already, setting the remaining bits to zero.
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

    /// Save current write position.
    pub fn save_state(&self) -> BufWriterState {
        BufWriterState {
            byte_idx: self.byte_idx,
            bit_idx: self.bit_idx,
            len_bytes: self.len_bytes,
        }
    }

    /// Restore previous write position.
    /// Primary use case is to go back and update a boolean flag.
    /// Warning: writing dynamic objects after restoring state will corrupt previously dynamic objects.
    pub fn restore_state(&mut self, state: BufWriterState) {
        self.byte_idx = state.byte_idx;
        self.bit_idx = state.bit_idx;
        self.len_bytes = state.len_bytes;
    }
}

impl BufWriterState {
    pub(crate) fn bits_in_byte_left(&self) -> u8 {
        if self.byte_idx >= self.len_bytes {
            return 0;
        }
        self.bit_idx + 1
    }
}

impl UnsizedBuilder {
    pub fn new(wr: &mut BufWriter<'_>) -> Result<Self, Error> {
        // ensure start_idx below is on a byte boundary
        wr.align_byte();
        // reserve one size slot
        let size_slot_pos = wr.write_rev_len(0)?;
        let unsized_start_idx = wr.pos().0;
        Ok(UnsizedBuilder {
            size_slot_pos,
            unsized_start_idx,
        })
    }

    pub fn finish(self, wr: &mut BufWriter<'_>) -> Result<(), Error> {
        // T might have written several nib16_rev's as well, encode and place them after type's data
        wr.encode_len_fifo(wr.rev_len_pos(), self.size_slot_pos)?;
        // e.g., enum, only one nib discriminant is written => need to align
        wr.align_byte();
        let size_bytes = wr.pos().0 - self.unsized_start_idx;
        // write actual Unsized size, it will be encoded later, by the parent write method or when finish is called
        wr.update_rev_len(self.size_slot_pos, size_bytes)?;
        Ok(())
    }
}

#[derive(Debug, Copy, Clone)]
pub struct RevPos(usize);

#[cfg(test)]
mod tests {
    use crate::{
        BufReader, BufWriter, DeserializeShrinkWrap, ElementSize, Error, Nibble,
        SerializeShrinkWrap,
    };
    use hex_literal::hex;

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
        wr.write_nib(Nibble::new_masked(0b1010)).unwrap();
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
        wr.write_rev_len(3).unwrap();
        wr.write_rev_len(5).unwrap();
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
        wr.write_rev_len(3).unwrap();
        wr.write_rev_len(5).unwrap();
        wr.write_rev_len(7).unwrap();
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
        wr.write_rev_len(5).unwrap();
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

    #[test]
    fn u4_rev_overlap() {
        let mut buf = [0u8; 64];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_rev_len(1).unwrap();
        wr.write_u8(0x10).unwrap();
        wr.write_bool(true).unwrap();
        let buf = wr.finish().unwrap();
        assert_eq!(buf, hex!("10 81"))
    }

    #[test]
    fn un_rev_overlap() {
        let mut buf = [0u8; 64];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_rev_len(3).unwrap();
        wr.write_un8(3, 1).unwrap();
        wr.write_bool(false).unwrap();
        wr.write_unib32(0).unwrap();
        wr.write_un8(4, 5).unwrap();
        wr.write_un8(5, 5).unwrap();
        wr.write_un32(17, 58_800).unwrap();
        wr.write_bool(false).unwrap();
        let buf = wr.finish().unwrap();
        assert_eq!(buf, hex!("20 52 B9 6C 03"))
    }

    #[test]
    fn integers() {
        let mut buf = [0; 64];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_u8(120).unwrap();
        wr.write_u16(420).unwrap();
        wr.write_u32(1_048_576).unwrap();
        wr.write_u64(u64::MAX - 123).unwrap();
        wr.write_u128(u128::MAX - 256).unwrap();
        let buf = wr.finish().unwrap();
        assert_eq!(
            buf,
            hex!("78 A401 00001000 84FFFFFFFFFFFFFFFF FEFFFFFFFFFFFFFFFFFFFFFFFFFFFF")
        );

        let mut buf = [0; 64];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_i8(-120).unwrap();
        wr.write_i16(-420).unwrap();
        wr.write_i32(-1_048_576).unwrap();
        wr.write_i64(i64::MIN + 123).unwrap();
        wr.write_i128(i128::MIN + 256).unwrap();
        let buf = wr.finish().unwrap();
        assert_eq!(
            buf,
            hex!("88 5CFE 0000F0FF 7B00000000000080 00010000000000000000000000000080")
        );
    }

    #[test]
    fn floats() {
        let mut buf = [0u8; 16];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_f32(1.5f32).unwrap();
        wr.write_f64(-2.5f64).unwrap();
        let buf = wr.finish().unwrap();
        assert_eq!(&buf[0..4], &1.5f32.to_bits().to_le_bytes());
        assert_eq!(&buf[4..12], &(-2.5f64).to_bits().to_le_bytes());
    }

    #[test]
    fn write_bool_out_of_bounds() {
        let mut buf = [0u8; 1];
        let mut wr = BufWriter::new(&mut buf);
        for _ in 0..8 {
            wr.write_bool(true).unwrap();
        }
        assert_eq!(wr.write_bool(true), Err(Error::OutOfBoundsWriteBool));
    }

    #[test]
    fn write_nib_out_of_bounds() {
        let mut buf = [0u8; 1];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_nib(Nibble::new_masked(1)).unwrap();
        wr.write_nib(Nibble::new_masked(2)).unwrap();
        assert_eq!(
            wr.write_nib(Nibble::new_masked(3)),
            Err(Error::OutOfBoundsWriteU4)
        );
    }

    #[test]
    fn write_nib_masked_matches_write_nib() {
        let mut buf = [0u8; 1];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_nib_masked(0xFA).unwrap();
        let buf = wr.finish().unwrap();
        assert_eq!(buf, &[0b1010_0000]);
    }

    #[test]
    fn write_u8_out_of_bounds() {
        let mut buf = [0u8; 1];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_u8(0x11).unwrap();
        assert_eq!(wr.write_u8(0x22), Err(Error::OutOfBoundsWriteU8));
    }

    #[test]
    fn write_rev_len_out_of_bounds() {
        let mut buf = [0u8; 1];
        let mut wr = BufWriter::new(&mut buf);
        assert!(matches!(wr.write_rev_len(5), Err(Error::OutOfBoundsRev)));
    }

    #[test]
    fn rev_len_pos_matches_write() {
        let mut buf = [0u8; 4];
        let mut wr = BufWriter::new(&mut buf);
        assert_eq!(wr.rev_len_pos().0, 4);
        let pos = wr.write_rev_len(1).unwrap();
        assert_eq!(pos.0, 2);
        assert_eq!(wr.rev_len_pos().0, 2);
    }

    #[test]
    fn update_rev_len_success() {
        let mut buf = [0u8; 4];
        let mut wr = BufWriter::new(&mut buf);
        let pos = wr.write_rev_len(0xAABB).unwrap();
        wr.update_rev_len(pos, 0x1234).unwrap();
        assert_eq!(&wr.buf()[2..4], &[0x34, 0x12]);
    }

    #[test]
    fn update_rev_len_out_of_bounds() {
        let mut buf = [0u8; 4];
        let mut wr = BufWriter::new(&mut buf);
        let pos = wr.rev_len_pos();
        assert_eq!(wr.update_rev_len(pos, 1), Err(Error::OutOfBoundsRev));
    }

    #[test]
    fn write_raw_slice_out_of_bounds() {
        let mut buf = [0u8; 2];
        let mut wr = BufWriter::new(&mut buf);
        assert_eq!(
            wr.write_raw_slice(&[1, 2, 3]),
            Err(Error::OutOfBoundsWriteRawSlice)
        );
    }

    #[test]
    fn fill_nibbles_fills_remaining_buffer() {
        let mut buf = [0u8; 4];
        let mut wr = BufWriter::new(&mut buf);
        wr.fill_nibbles(0xA);
        assert_eq!(wr.buf(), &[0xAA, 0xAA, 0xAA, 0xAA]);
    }

    #[test]
    fn fill_nibbles_noop_when_full() {
        let mut buf = [0x11u8; 1];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_u8(0x11).unwrap();
        wr.fill_nibbles(0xA);
        assert_eq!(wr.buf(), &[0x11]);
    }

    #[test]
    fn fill_bytes_noop_when_full() {
        let mut buf = [0x11u8; 1];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_u8(0x11).unwrap();
        wr.fill_bytes(0xCC);
        assert_eq!(wr.buf(), &[0x11]);
    }

    #[test]
    fn write_bytes_direct() {
        let mut buf = [0u8; 8];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_bytes(&[1, 2, 3]).unwrap();
        let buf = wr.finish().unwrap();
        assert_eq!(buf, &[1, 2, 3, 0x03]);
    }

    #[test]
    fn write_bytes_too_long() {
        let data = vec![0u8; u16::MAX as usize + 1];
        let mut buf = [0u8; 4];
        let mut wr = BufWriter::new(&mut buf);
        assert_eq!(wr.write_bytes(&data), Err(Error::LenTooLong));
    }

    #[test]
    fn write_str_direct() {
        let mut buf = [0u8; 8];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_str("ab").unwrap();
        let buf = wr.finish().unwrap();
        assert_eq!(buf, &[b'a', b'b', 0x02]);
    }

    #[test]
    fn write_str_too_long() {
        let data = "a".repeat(u16::MAX as usize + 1);
        let mut buf = [0u8; 4];
        let mut wr = BufWriter::new(&mut buf);
        assert_eq!(wr.write_str(&data), Err(Error::LenTooLong));
    }

    /// Minimal Unsized type used to exercise [BufWriter::write] and [UnsizedBuilder].
    struct DummyUnsized(u8);

    impl SerializeShrinkWrap for DummyUnsized {
        const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

        fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
            wr.write_u8(self.0)
        }
    }

    impl<'i> DeserializeShrinkWrap<'i> for DummyUnsized {
        const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

        fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
            Ok(DummyUnsized(rd.read_u8()?))
        }
    }

    #[test]
    fn write_unsized_round_trip() {
        let mut buf = [0u8; 16];
        let mut wr = BufWriter::new(&mut buf);
        wr.write(&DummyUnsized(0xAB)).unwrap();
        let bytes = wr.finish().unwrap();
        let mut rd = BufReader::new(bytes);
        let decoded: DummyUnsized = rd.read().unwrap();
        assert_eq!(decoded.0, 0xAB);
    }

    #[test]
    fn encode_nib16_rev_noop_when_to_before_from() {
        let mut buf = [0u8; 8];
        let mut wr = BufWriter::new(&mut buf);
        let earlier = wr.rev_len_pos(); // 8, less consumed
        wr.write_rev_len(1).unwrap();
        let later = wr.rev_len_pos(); // 6, more consumed
        // Passing them swapped (to.0 < from.0) must be a no-op.
        assert!(wr.encode_len_fifo(earlier, later).is_ok());
    }

    #[test]
    fn encode_nib16_rev_noop_when_equal() {
        let mut buf = [0u8; 8];
        let mut wr = BufWriter::new(&mut buf);
        let pos = wr.rev_len_pos();
        assert!(wr.encode_len_fifo(pos, pos).is_ok());
    }

    #[test]
    fn finish_and_take_returns_slice() {
        let mut buf = [0u8; 4];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_u8(0xAB).unwrap();
        let bytes = wr.finish_and_take().unwrap();
        assert_eq!(bytes, &[0xAB]);
    }

    #[test]
    fn deinit_returns_buffer() {
        let mut buf = [0u8; 4];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_u8(0xAB).unwrap();
        let raw = wr.deinit();
        assert_eq!(raw, &[0xAB, 0, 0, 0]);
    }

    #[test]
    fn buf_accessor() {
        let mut buf = [0u8; 4];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_u8(0xAB).unwrap();
        assert_eq!(wr.buf(), &[0xAB, 0, 0, 0]);
    }

    #[test]
    fn align_nibble_noop_at_boundary() {
        let mut buf = [0u8; 2];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_nib(Nibble::new_masked(0xA)).unwrap();
        wr.align_nibble();
        assert_eq!(wr.pos(), (0, 3));
    }

    #[test]
    fn align_nibble_from_lower_bits() {
        let mut buf = [0u8; 2];
        let mut wr = BufWriter::new(&mut buf);
        for _ in 0..5 {
            wr.write_bool(true).unwrap();
        }
        assert_eq!(wr.pos(), (0, 2));
        wr.align_nibble();
        assert_eq!(wr.pos(), (1, 7));
        let buf = wr.finish().unwrap();
        assert_eq!(buf, &[0b1111_1000]);
    }

    #[test]
    fn bytes_left_partial_byte() {
        let mut buf = [0u8; 4];
        let mut wr = BufWriter::new(&mut buf);
        assert_eq!(wr.bytes_left(), 4);
        wr.write_bool(true).unwrap();
        assert_eq!(wr.bytes_left(), 3);
    }

    #[test]
    fn nibbles_left_at_nibble_boundary() {
        let mut buf = [0u8; 2];
        let mut wr = BufWriter::new(&mut buf);
        assert_eq!(wr.nibbles_left(), 4);
        wr.write_nib(Nibble::new_masked(0xA)).unwrap();
        assert_eq!(wr.nibbles_left(), 3);
    }

    #[test]
    fn pos_tracks_progress() {
        let mut buf = [0u8; 4];
        let mut wr = BufWriter::new(&mut buf);
        assert_eq!(wr.pos(), (0, 7));
        wr.write_bool(true).unwrap();
        assert_eq!(wr.pos(), (0, 6));
        // write_u8 aligns to the next byte first (abandoning the rest of the current one),
        // then writes into it, so byte_idx advances by two here, not one.
        wr.write_u8(0xFF).unwrap();
        assert_eq!(wr.pos(), (2, 7));
    }

    #[test]
    fn save_restore_state_updates_flag() {
        let mut buf = [0u8; 4];
        let mut wr = BufWriter::new(&mut buf);
        let state_before_flag = wr.save_state();
        wr.write_bool(false).unwrap(); // placeholder flag
        wr.write_u8(0x42).unwrap();
        let state_after = wr.save_state();
        wr.restore_state(state_before_flag);
        wr.write_bool(true).unwrap(); // update flag to its real value
        wr.restore_state(state_after); // resume where we left off
        let buf = wr.finish().unwrap();
        assert_eq!(buf, &[0b1000_0000, 0x42]);
    }

    #[test]
    fn reset_clears_progress() {
        let mut buf = [0u8; 4];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_u8(0xAB).unwrap();
        wr.reset();
        assert_eq!(wr.pos(), (0, 7));
        assert_eq!(wr.bytes_left(), 4);
        wr.write_u8(0xCD).unwrap();
        assert_eq!(wr.finish().unwrap(), &[0xCD]);
    }

    #[test]
    fn buf_writer_state_bits_in_byte_left() {
        let mut buf = [0u8; 2];
        let mut wr = BufWriter::new(&mut buf);
        assert_eq!(wr.save_state().bits_in_byte_left(), 8);
        wr.write_bool(true).unwrap();
        assert_eq!(wr.save_state().bits_in_byte_left(), 7);
        wr.write_u8(0).unwrap();
        assert_eq!(wr.save_state().bits_in_byte_left(), 0);
    }

    /// Unsized type whose body is bigger than u16::MAX bytes, used to exercise
    /// [UnsizedBuilder::finish]'s ItemTooLong error path.
    struct HugeUnsized;

    impl SerializeShrinkWrap for HugeUnsized {
        const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

        fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
            let data = vec![0u8; u16::MAX as usize + 1];
            wr.write_raw_slice(&data)
        }
    }

    #[test]
    fn write_unsized_item_too_long() {
        let mut buf = vec![0u8; u16::MAX as usize + 16];
        let mut wr = BufWriter::new(&mut buf);
        assert_eq!(wr.write(&HugeUnsized), Err(Error::LenTooLong));
    }
}

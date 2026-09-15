use crate::nib32::UNib32;
use crate::{ElementSize, Error, Nibble, SerializeShrinkWrapOwned};

const ONE_MORE_NIBBLE: u8 = 0b1000;

/// Growable buffer writer, backed by [Vec], with the same wire format as [BufWriter](crate::BufWriter).
///
/// Differences from [BufWriter](crate::BufWriter):
/// * The byte buffer grows as needed, so forward writes never fail with out-of-bounds errors.
/// * Reverse (length) numbers are kept in a separate `Vec<u32>` FIFO instead of the back of the byte buffer,
///   so lengths are not limited to `u16::MAX`.
/// * No `save_state` / `restore_state`: with an allocator available, simply serialize into a
///   temporary writer instead.
///
/// # Example
/// ```
/// let mut wr = shrink_wrap::BufWriterOwned::new();
/// wr.write_bool(true).unwrap();
/// wr.write_u8(0xaa).unwrap();
/// let bytes = wr.finish().unwrap();
/// assert_eq!(bytes, &[0x80, 0xaa]);
/// ```
#[derive(Default, Debug, Clone)]
pub struct BufWriterOwned {
    buf: Vec<u8>,
    /// Next byte to write to, invariant: `buf.len() == byte_idx` if `bit_idx == 7`, else `buf.len() == byte_idx + 1`.
    byte_idx: usize,
    /// Next bit to write to, starts from 7
    bit_idx: u8,
    /// FIFO of numbers to be encoded in UNib32 reverse encoding.
    rev: Vec<u32>,
}

/// Builder-style serialization token for 'Unsized' types.
pub struct UnsizedBuilderOwned {
    size_slot_pos: RevPos,
    unsized_start_idx: usize,
}

/// Index of a number in the reverse FIFO of [BufWriterOwned].
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct RevPos(usize);

impl BufWriterOwned {
    /// Create a new empty BufWriterOwned.
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            byte_idx: 0,
            bit_idx: 7,
            rev: Vec::new(),
        }
    }

    /// Create a new empty BufWriterOwned with pre-allocated capacity for the byte buffer.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: Vec::with_capacity(capacity),
            byte_idx: 0,
            bit_idx: 7,
            rev: Vec::new(),
        }
    }

    /// Reset BufWriterOwned to the beginning, "forgetting" all written data, but keeping allocated capacity.
    /// Useful when re-using the same writer multiple times.
    pub fn reset(&mut self) {
        self.buf.clear();
        self.rev.clear();
        self.byte_idx = 0;
        self.bit_idx = 7;
    }

    /// Ensure the byte at `byte_idx` exists (pushing a zero byte when starting a new one).
    #[inline]
    fn ensure_byte(&mut self) {
        if self.byte_idx == self.buf.len() {
            self.buf.push(0);
        }
    }

    /// Write one bit to the buffer. One can write 8 bits with this function, and only one byte will be used in the buffer.
    /// Nibble writes will align the buffer to nibble boundary and byte writes to byte boundary.
    pub fn write_bool(&mut self, val: bool) -> Result<(), Error> {
        self.ensure_byte();
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
        self.ensure_byte();
        if self.bit_idx == 7 {
            self.buf[self.byte_idx] |= val.value() << 4;
            self.bit_idx = 3;
        } else {
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

    fn write_un_inner(&mut self, bit_count: u8, value: u64) -> Result<(), Error> {
        let mut bits_left = bit_count;
        while bits_left > 0 {
            self.ensure_byte();
            let bits_to_write = bits_left.min(self.bit_idx + 1);
            let mask = (1u64 << bits_to_write) - 1;
            let bits = ((value >> (bits_left - bits_to_write)) & mask) as u8;
            self.buf[self.byte_idx] |= bits << (self.bit_idx + 1 - bits_to_write);

            self.bit_idx = self.bit_idx.wrapping_sub(bits_to_write);
            if self.bit_idx == 255 {
                self.bit_idx = 7;
                self.byte_idx += 1;
            }
            bits_left -= bits_to_write;
        }
        Ok(())
    }

    /// Write up to 8 bits from u8 number without alignment.
    pub fn write_un8(&mut self, bit_count: u8, value: u8) -> Result<(), Error> {
        if bit_count > 8 {
            return Err(Error::InvalidBitCount);
        }
        self.write_un_inner(bit_count, value as u64)
    }

    /// Write up to 16 bits from u16 number without alignment.
    pub fn write_un16(&mut self, bit_count: u8, value: u16) -> Result<(), Error> {
        if bit_count > 16 {
            return Err(Error::InvalidBitCount);
        }
        self.write_un_inner(bit_count, value as u64)
    }

    /// Write up to 32 bits from u32 number without alignment.
    pub fn write_un32(&mut self, bit_count: u8, value: u32) -> Result<(), Error> {
        if bit_count > 32 {
            return Err(Error::InvalidBitCount);
        }
        self.write_un_inner(bit_count, value as u64)
    }

    /// Write up to 64 bits from u64 number without alignment.
    pub fn write_un64(&mut self, bit_count: u8, value: u64) -> Result<(), Error> {
        if bit_count > 64 {
            return Err(Error::InvalidBitCount);
        }
        self.write_un_inner(bit_count, value)
    }

    /// Write u8 with alignment of 1 byte.
    pub fn write_u8(&mut self, val: u8) -> Result<(), Error> {
        self.align_byte();
        self.buf.push(val);
        self.byte_idx += 1;
        Ok(())
    }

    /// Write u16 in Little Endian and alignment of 1 byte.
    pub fn write_u16(&mut self, val: u16) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())
    }

    /// Write u32 in UNib32 forward encoding. It will take from 1 nibble to 11 nibbles in the buffer,
    /// depending on the number. Alignment of 4 bits is used.
    pub fn write_unib32(&mut self, val: u32) -> Result<(), Error> {
        let mut val = val;
        let mut nibbles_left = UNib32(val).len_nibbles();
        while nibbles_left > 0 {
            let nib = (val & 0b111) as u8;
            let nib = if nibbles_left > 1 {
                nib | ONE_MORE_NIBBLE
            } else {
                nib
            };
            self.write_nib_masked(nib)?;
            val >>= 3;
            nibbles_left -= 1;
        }
        Ok(())
    }

    /// Write u32 in UNib32 reverse encoding at the current position.
    fn write_unib32_reversed(&mut self, val: u32) -> Result<(), Error> {
        let mut val = val;
        let len = UNib32(val).len_nibbles();
        for i in 0..len {
            let nib = (val & 0b111) as u8;
            // reversed unib is written left to right, but read from right to left, so "one more nibble" bits must also be reversed
            if i == 0 {
                self.write_nib_masked(nib)?;
            } else {
                self.write_nib_masked(nib | ONE_MORE_NIBBLE)?;
            }
            val >>= 3;
        }
        Ok(())
    }

    /// Push u32 to the reverse FIFO, later when [encode_len_fifo](Self::encode_len_fifo) or [finish](Self::finish)
    /// are called, all the numbers will be encoded to UNib32 reverse encoding.
    pub fn write_rev_len(&mut self, len: usize) -> Result<RevPos, Error> {
        let len = u32::try_from(len).map_err(|_| Error::LenTooLong)?;
        self.rev.push(len);
        Ok(RevPos(self.rev.len() - 1))
    }

    /// Current top of the reverse FIFO (index of the next number to be pushed).
    /// See [encode_nib32_rev](Self::encode_nib32_rev) on how this function is used.
    pub fn rev_len_pos(&self) -> RevPos {
        RevPos(self.rev.len())
    }

    /// Update previously pushed u32 value in the reverse FIFO, using the obtained index.
    pub fn update_rev_len(&mut self, pos: RevPos, len: usize) -> Result<(), Error> {
        let Some(slot) = self.rev.get_mut(pos.0) else {
            return Err(Error::OutOfBoundsRev);
        };
        let len = u32::try_from(len).map_err(|_| Error::LenTooLong)?;
        *slot = len;
        #[cfg(feature = "tracing-extended")]
        tracing::trace!("updated u32 rev at pos{} = {val}", pos.0);
        Ok(())
    }

    /// Write u32 in Little Endian and alignment of 1 byte.
    pub fn write_u32(&mut self, val: u32) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())
    }

    /// Write u64 in Little Endian and alignment of 1 byte.
    pub fn write_u64(&mut self, val: u64) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())
    }

    /// Write u128 in Little Endian and alignment of 1 byte.
    pub fn write_u128(&mut self, val: u128) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())
    }

    /// Write i8 and alignment of 1 byte.
    pub fn write_i8(&mut self, val: i8) -> Result<(), Error> {
        self.write_u8(val as u8)
    }

    /// Write i16 in Little Endian and alignment of 1 byte.
    pub fn write_i16(&mut self, val: i16) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())
    }

    /// Write i32 in Little Endian and alignment of 1 byte.
    pub fn write_i32(&mut self, val: i32) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())
    }

    /// Write i64 in Little Endian and alignment of 1 byte.
    pub fn write_i64(&mut self, val: i64) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())
    }

    /// Write i128 in Little Endian and alignment of 1 byte.
    pub fn write_i128(&mut self, val: i128) -> Result<(), Error> {
        self.write_raw_slice(&val.to_le_bytes())
    }

    /// Write f32 in Little Endian and alignment of 1 byte.
    pub fn write_f32(&mut self, val: f32) -> Result<(), Error> {
        self.write_raw_slice(&val.to_bits().to_le_bytes())
    }

    /// Write f64 in Little Endian and alignment of 1 byte.
    pub fn write_f64(&mut self, val: f64) -> Result<(), Error> {
        self.write_raw_slice(&val.to_bits().to_le_bytes())
    }

    /// Write the provided slice to the buffer as is. Note that you won't be able to read it back
    /// with BufReader without knowing the length, which is not written in this case.
    /// See also [Self::write_bytes].
    pub fn write_raw_slice(&mut self, val: &[u8]) -> Result<(), Error> {
        self.align_byte();
        self.buf.extend_from_slice(val);
        self.byte_idx += val.len();
        Ok(())
    }

    /// Write variable length slice to the buffer. Length will be pushed to the reverse FIFO.
    pub fn write_bytes(&mut self, val: &[u8]) -> Result<(), Error> {
        self.write_rev_len(val.len())?;
        self.write_raw_slice(val)
    }

    /// Write variable length string to the buffer. Length will be pushed to the reverse FIFO.
    pub fn write_str(&mut self, val: &str) -> Result<(), Error> {
        self.write_rev_len(val.len())?;
        self.write_raw_slice(val.as_bytes())
    }

    /// Write any value that implements [SerializeShrinkWrapOwned].
    ///
    /// If the value is Unsized, then size is calculated and pushed to the reverse FIFO as u32,
    /// which is later encoded to reverse UNib32.
    ///
    /// Note that for serializing root structs or enums, it's better to call ser_shrink_wrap_owned directly,
    /// as it avoids wasting space for the object size, which is known from the buffer size itself.
    ///
    /// Values serialized with this method must be deserialized with [read](crate::BufReader::read)
    /// or [read_owned](crate::BufReader::read_owned).
    pub fn write<T: SerializeShrinkWrapOwned>(&mut self, val: &T) -> Result<(), Error> {
        let unsized_builder = if matches!(T::ELEMENT_SIZE, ElementSize::Unsized) {
            Some(UnsizedBuilderOwned::new(self)?)
        } else {
            None
        };
        val.ser_shrink_wrap_owned(self)?;
        if let Some(builder) = unsized_builder {
            builder.finish(self)?;
        }
        Ok(())
    }

    /// Encode numbers from the reverse FIFO with indices in `to.0 + 1 .. from.0` (i.e. everything pushed
    /// after slot `to`, up to but not including `from`) into UNib32 reverse encoding at the current position,
    /// newest first, and remove them from the FIFO. Slot `to` itself is left untouched.
    /// No-op if `from.0 <= to.0 + 1`.
    ///
    /// This operation allows preserving backwards and forwards compatibility:
    /// * newer data read by old code: additional bytes can be ignored.
    /// * old data read by new code: missing bytes are expected and None or 0 length arrays are created.
    ///
    /// Example of how this function is used when serializing a variable length object:
    /// ```
    /// use shrink_wrap::BufWriterOwned;
    /// let mut wr = BufWriterOwned::new();
    ///
    /// let size_slot_pos = wr.write_u32_rev(0).unwrap(); // reserve a size slot in the reverse FIFO
    /// let unsized_start_bytes = wr.pos().0; // remember current position in bytes
    /// // Write an object of unknown size, potentially containing more objects with variable length,
    /// // which in turn will push more numbers to the reverse FIFO.
    /// wr.write_bytes(&[1u8, 2, 3]).unwrap();
    /// // Encode numbers pushed by the object itself to UNib32 reverse encoding, if any
    /// wr.encode_nib32_rev(wr.u32_rev_pos(), size_slot_pos).unwrap();
    /// wr.align_byte(); // Variable sized objects must be byte aligned
    /// let size_bytes = wr.pos().0 - unsized_start_bytes;
    /// assert_eq!(size_bytes, 4);
    /// // Update the original slot with an actual size.
    /// wr.update_u32_rev(size_slot_pos, size_bytes as u32).unwrap();
    /// let buf = wr.finish().unwrap();
    /// assert_eq!(buf, &[1, 2, 3, 3, 4]);
    /// ```
    pub fn encode_len_fifo(&mut self, from: RevPos, to: RevPos) -> Result<(), Error> {
        if from.0 <= to.0 {
            return Ok(());
        }
        self.encode_rev_range(to.0 + 1, from.0)
    }

    /// Encode reverse FIFO entries with indices in `start..end` (newest first) and remove them.
    fn encode_rev_range(&mut self, start: usize, end: usize) -> Result<(), Error> {
        let end = end.min(self.rev.len());
        if start >= end {
            return Ok(());
        }
        let mut total_nibbles: usize = self.rev[start..end]
            .iter()
            .map(|v| UNib32(*v).len_nibbles())
            .sum();
        self.align_nibble();
        if self.bit_idx != 7 {
            total_nibbles += 1;
        }
        if !total_nibbles.is_multiple_of(2) {
            // ensure that reading from the back always starts from a valid UNib32
            self.write_nib(Nibble::zero())?;
        }
        for val in self.rev.drain(start..end).rev().collect::<Vec<u32>>() {
            self.write_unib32_reversed(val)?;
            #[cfg(feature = "tracing-extended")]
            tracing::trace!("encoded rev.UNib32 = {val}");
        }
        debug_assert!(self.bit_idx == 7);
        Ok(())
    }

    /// Encode all the remaining numbers in the reverse FIFO, align to byte and return the slice containing written data.
    ///
    /// The writer is not reset, call [reset](Self::reset) before re-using it.
    pub fn finish(&mut self) -> Result<&[u8], Error> {
        if self.rev.is_empty() {
            self.align_byte();
        } else {
            self.encode_rev_range(0, self.rev.len())?;
        }
        Ok(&self.buf)
    }

    /// Encode all the remaining numbers in the reverse FIFO, align to byte and return the Vec containing written data.
    pub fn finish_and_take(mut self) -> Result<Vec<u8>, Error> {
        self.finish()?;
        Ok(self.buf)
    }

    /// Return the underlying buffer without encoding the reverse FIFO.
    pub fn deinit(self) -> Vec<u8> {
        self.buf
    }

    /// Bytes written so far (including a partially filled last byte, if any).
    pub fn buf(&self) -> &[u8] {
        &self.buf
    }

    /// Align writer to the next nibble if not already, remaining bits are already zero.
    #[inline]
    pub fn align_nibble(&mut self) {
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

    /// Align writer to the next byte if not already, remaining bits are already zero.
    #[inline]
    pub fn align_byte(&mut self) {
        if self.bit_idx == 7 {
            return;
        }
        self.bit_idx = 7;
        self.byte_idx += 1;
    }

    /// Return the current position in bytes and bits.
    #[inline]
    pub fn pos(&self) -> (usize, u8) {
        (self.byte_idx, self.bit_idx)
    }
}

impl UnsizedBuilderOwned {
    pub fn new(wr: &mut BufWriterOwned) -> Result<Self, Error> {
        // ensure start_idx below is on a byte boundary
        wr.align_byte();
        // reserve one size slot
        let size_slot_pos = wr.write_rev_len(0)?;
        let unsized_start_idx = wr.pos().0;
        Ok(UnsizedBuilderOwned {
            size_slot_pos,
            unsized_start_idx,
        })
    }

    pub fn finish(self, wr: &mut BufWriterOwned) -> Result<(), Error> {
        // T might have pushed several rev numbers as well, encode and place them after type's data
        wr.encode_len_fifo(wr.rev_len_pos(), self.size_slot_pos)?;
        // e.g., enum, only one nib discriminant is written => need to align
        wr.align_byte();
        let size_bytes = wr.pos().0 - self.unsized_start_idx;
        // write actual Unsized size, it will be encoded later, by the parent or when finish is called
        wr.update_rev_len(self.size_slot_pos, size_bytes)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BufReader, BufWriter, Error, Nibble, UNib32};
    use hex_literal::hex;

    #[test]
    fn booleans() {
        let mut wr = BufWriterOwned::new();
        for b in [true, false, true, false, true, true, false, false] {
            wr.write_bool(b).unwrap();
        }
        assert_eq!(wr.finish().unwrap(), &[0b10101100]);
    }

    #[test]
    fn write_u8_after_bits() {
        let mut wr = BufWriterOwned::new();
        wr.write_bool(true).unwrap();
        wr.write_bool(false).unwrap();
        wr.write_u8(0xAA).unwrap();
        assert_eq!(wr.finish().unwrap(), &[0b1000_0000, 0xAA]);
    }

    #[test]
    fn nibble_after_bits() {
        let mut wr = BufWriterOwned::new();
        wr.write_bool(true).unwrap();
        wr.write_bool(false).unwrap();
        wr.write_nib(Nibble::new_masked(0b1010)).unwrap();
        assert_eq!(wr.finish().unwrap(), &[0b1000_1010]);
    }

    #[test]
    fn rev_aligned() {
        let mut wr = BufWriterOwned::new();
        wr.write_u8(0xAA).unwrap();
        wr.write_u8(0xCC).unwrap();
        wr.write_rev_len(3).unwrap();
        wr.write_rev_len(5).unwrap();
        assert_eq!(wr.finish().unwrap(), &[0xAA, 0xCC, 0b0101_0011]);
    }

    #[test]
    fn rev_unaligned() {
        let mut wr = BufWriterOwned::new();
        wr.write_u8(0xAA).unwrap();
        wr.write_u8(0xCC).unwrap();
        wr.write_rev_len(3).unwrap();
        wr.write_rev_len(5).unwrap();
        wr.write_rev_len(7).unwrap();
        assert_eq!(
            wr.finish().unwrap(),
            &[0xAA, 0xCC, 0b0000_0111, 0b0101_0011]
        );
    }

    #[test]
    fn rev_smallest() {
        let mut wr = BufWriterOwned::new();
        wr.write_unib32(2).unwrap();
        wr.write_rev_len(5).unwrap();
        assert_eq!(wr.finish().unwrap(), &[0x25]);
    }

    #[test]
    fn write_un() {
        let mut wr = BufWriterOwned::new();
        wr.write_bool(true).unwrap();
        wr.write_un8(7, 0b010_1010).unwrap();
        wr.write_un8(3, 0b110).unwrap();
        wr.write_un16(12, 0b1011_1001_0100).unwrap();
        wr.write_un32(17, 0b1_10101111_01010011).unwrap();
        assert_eq!(
            wr.finish().unwrap(),
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
    fn write_un_invalid_bit_count() {
        let mut wr = BufWriterOwned::new();
        assert_eq!(wr.write_un8(9, 0), Err(Error::InvalidBitCount));
        assert_eq!(wr.write_un64(65, 0), Err(Error::InvalidBitCount));
    }

    #[test]
    fn un_rev_overlap() {
        let mut wr = BufWriterOwned::new();
        wr.write_rev_len(3).unwrap();
        wr.write_un8(3, 1).unwrap();
        wr.write_bool(false).unwrap();
        wr.write_unib32(0).unwrap();
        wr.write_un8(4, 5).unwrap();
        wr.write_un8(5, 5).unwrap();
        wr.write_un32(17, 58_800).unwrap();
        wr.write_bool(false).unwrap();
        assert_eq!(wr.finish().unwrap(), hex!("20 52 B9 6C 03"))
    }

    #[test]
    fn integers() {
        let mut wr = BufWriterOwned::new();
        wr.write_u8(120).unwrap();
        wr.write_u16(420).unwrap();
        wr.write_u32(1_048_576).unwrap();
        wr.write_u64(u64::MAX - 123).unwrap();
        wr.write_u128(u128::MAX - 256).unwrap();
        assert_eq!(
            wr.finish().unwrap(),
            hex!("78 A401 00001000 84FFFFFFFFFFFFFFFF FEFFFFFFFFFFFFFFFFFFFFFFFFFFFF")
        );

        let mut wr = BufWriterOwned::new();
        wr.write_i8(-120).unwrap();
        wr.write_i16(-420).unwrap();
        wr.write_i32(-1_048_576).unwrap();
        wr.write_i64(i64::MIN + 123).unwrap();
        wr.write_i128(i128::MIN + 256).unwrap();
        assert_eq!(
            wr.finish().unwrap(),
            hex!("88 5CFE 0000F0FF 7B00000000000080 00010000000000000000000000000080")
        );
    }

    #[test]
    fn floats() {
        let mut wr = BufWriterOwned::new();
        wr.write_f32(1.5f32).unwrap();
        wr.write_f64(-2.5f64).unwrap();
        let buf = wr.finish().unwrap();
        assert_eq!(&buf[0..4], &1.5f32.to_bits().to_le_bytes());
        assert_eq!(&buf[4..12], &(-2.5f64).to_bits().to_le_bytes());
    }

    #[test]
    fn write_bytes_and_str() {
        let mut wr = BufWriterOwned::new();
        wr.write_bytes(&[1, 2, 3]).unwrap();
        assert_eq!(wr.finish().unwrap(), &[1, 2, 3, 0x03]);

        let mut wr = BufWriterOwned::new();
        wr.write_str("ab").unwrap();
        assert_eq!(wr.finish().unwrap(), &[b'a', b'b', 0x02]);
    }

    #[test]
    fn write_bytes_over_u16() {
        let data = vec![0xEEu8; u16::MAX as usize + 1];
        let mut wr = BufWriterOwned::new();
        wr.write_bytes(&data).unwrap();
        let bytes = wr.finish().unwrap();
        let mut rd = BufReader::new(bytes);
        assert_eq!(rd.read_bytes().unwrap(), &data[..]);
    }

    #[test]
    fn update_u32_rev() {
        let mut wr = BufWriterOwned::new();
        let pos = wr.write_rev_len(0xAABB).unwrap();
        wr.update_rev_len(pos, 5).unwrap();
        assert_eq!(wr.finish().unwrap(), &[0x05]);
    }

    #[test]
    fn update_u32_rev_out_of_bounds() {
        let mut wr = BufWriterOwned::new();
        let pos = wr.rev_len_pos();
        assert_eq!(wr.update_rev_len(pos, 1), Err(Error::OutOfBoundsRev));
    }

    #[test]
    fn encode_nib32_rev_noop_when_swapped_or_equal() {
        let mut wr = BufWriterOwned::new();
        let earlier = wr.rev_len_pos();
        wr.write_rev_len(1).unwrap();
        let later = wr.rev_len_pos();
        wr.encode_len_fifo(earlier, later).unwrap();
        wr.encode_len_fifo(later, later).unwrap();
        assert_eq!(wr.buf(), &[]);
        assert_eq!(wr.rev_len_pos(), later);
    }

    #[test]
    fn unsized_builder_round_trip() {
        let mut wr = BufWriterOwned::new();
        let builder = UnsizedBuilderOwned::new(&mut wr).unwrap();
        wr.write_u8(0xAB).unwrap();
        wr.write_bytes(&[1, 2]).unwrap();
        builder.finish(&mut wr).unwrap();
        wr.write_u8(0xCD).unwrap();
        let bytes = wr.finish().unwrap();
        // 0xAB, 1, 2, rev(len=2) + pad, then 0xCD, then rev(size=4)
        assert_eq!(bytes, &[0xAB, 1, 2, 0x02, 0xCD, 0x04]);

        let mut rd = BufReader::new(bytes);
        let size = rd.read_rev_len().unwrap() as usize;
        let mut inner = rd.split(size).unwrap();
        assert_eq!(inner.read_u8().unwrap(), 0xAB);
        assert_eq!(inner.read_bytes().unwrap(), &[1, 2]);
        assert_eq!(rd.read_u8().unwrap(), 0xCD);
    }

    /// Minimal Unsized type used to exercise [BufWriterOwned::write] and [UnsizedBuilderOwned].
    struct DummyUnsized(u8);

    impl SerializeShrinkWrapOwned for DummyUnsized {
        const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

        fn ser_shrink_wrap_owned(&self, wr: &mut BufWriterOwned) -> Result<(), Error> {
            wr.write_u8(self.0)
        }
    }

    impl crate::DeserializeShrinkWrapOwned for DummyUnsized {
        const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

        fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, Error> {
            Ok(DummyUnsized(rd.read_u8()?))
        }
    }

    #[test]
    fn write_unsized_round_trip() {
        let mut wr = BufWriterOwned::new();
        wr.write(&DummyUnsized(0xAB)).unwrap();
        let bytes = wr.finish().unwrap();
        let mut rd = BufReader::new(bytes);
        let decoded: DummyUnsized = rd.read_owned().unwrap();
        assert_eq!(decoded.0, 0xAB);
    }

    type Mixed = (
        u8,
        Option<String>,
        Vec<Vec<u16>>,
        Result<bool, i32>,
        [u8; 2],
    );

    #[test]
    fn write_std_types_matches_buf_writer() {
        let value: Mixed = (
            7,
            Some("hi".to_string()),
            vec![vec![1, 2], vec![], vec![3]],
            Err(-5),
            [9, 8],
        );
        let mut buf = [0u8; 128];
        let mut wr = BufWriter::new(&mut buf);
        wr.write(&value).unwrap();
        let mut wro = BufWriterOwned::new();
        wro.write(&value).unwrap();
        assert_eq!(wr.finish().unwrap(), wro.finish().unwrap());

        let bytes = value.to_ww_bytes_owned().unwrap();
        let mut rd = BufReader::new(&bytes);
        let decoded: Mixed = rd.read_owned().unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn matches_buf_writer() {
        let mut buf = [0u8; 128];
        let mut wr = BufWriter::new(&mut buf);
        let mut wro = BufWriterOwned::new();

        wr.write_bool(true).unwrap();
        wro.write_bool(true).unwrap();
        wr.write_rev_len(7).unwrap();
        wro.write_rev_len(7).unwrap();
        wr.write_un16(11, 0x5A5).unwrap();
        wro.write_un16(11, 0x5A5).unwrap();
        wr.write_unib32(1234).unwrap();
        wro.write_unib32(1234).unwrap();
        wr.write_str("hello").unwrap();
        wro.write_str("hello").unwrap();
        wr.write_rev_len(0).unwrap();
        wro.write_rev_len(0).unwrap();
        wr.write_rev_len(65535).unwrap();
        wro.write_rev_len(65535).unwrap();
        wr.write_nib_masked(0xF).unwrap();
        wro.write_nib_masked(0xF).unwrap();

        assert_eq!(wr.finish().unwrap(), wro.finish().unwrap());
    }

    #[test]
    fn reversed_round_trip() {
        for num in [
            0,
            1,
            7,
            8,
            63,
            64,
            65_535,
            65_536,
            1 << 20,
            u32::MAX as usize,
        ] {
            let mut wr = BufWriterOwned::new();
            wr.write_rev_len(num).unwrap();
            let bytes = wr.finish().unwrap();
            let mut rd = BufReader::new(bytes);
            assert_eq!(rd.read_rev_len(), Ok(num));
        }
        let _ = UNib32(0);
    }

    #[test]
    fn reset_keeps_working() {
        let mut wr = BufWriterOwned::new();
        wr.write_u8(0xAB).unwrap();
        wr.write_rev_len(1).unwrap();
        wr.reset();
        assert_eq!(wr.pos(), (0, 7));
        wr.write_u8(0xCD).unwrap();
        assert_eq!(wr.finish().unwrap(), &[0xCD]);
    }

    #[test]
    fn finish_and_take_and_deinit() {
        let mut wr = BufWriterOwned::new();
        wr.write_bool(true).unwrap();
        assert_eq!(wr.finish_and_take().unwrap(), vec![0x80]);

        let mut wr = BufWriterOwned::with_capacity(8);
        wr.write_u8(0xAB).unwrap();
        assert_eq!(wr.deinit(), vec![0xAB]);
    }

    #[test]
    fn pos_tracks_progress() {
        let mut wr = BufWriterOwned::new();
        assert_eq!(wr.pos(), (0, 7));
        wr.write_bool(true).unwrap();
        assert_eq!(wr.pos(), (0, 6));
        wr.write_u8(0xFF).unwrap();
        assert_eq!(wr.pos(), (2, 7));
        assert_eq!(wr.buf(), &[0x80, 0xFF]);
    }
}

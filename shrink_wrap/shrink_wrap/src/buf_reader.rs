use crate::Error::OutOfBoundsRev;
use crate::nib32::UNib32;
use crate::un::read_unx;
use crate::vlq32::UVlq32;
use crate::vlq64::UVlq64;
use crate::{DeserializeShrinkWrap, DeserializeShrinkWrapOwned, ElementSize, Error, Nibble};

/// Buffer reader that treats input as a stream of bits, nibbles or bytes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct BufReader<'i> {
    buf: &'i [u8],
    /// Buffer length from the front, shrinks when read_vlu16n_rev() is used.
    len_bytes: usize,
    /// * Starts as false: read next u4_rev from bits 3:0
    /// * When true: read next u4_rev from bits 7:4 and decrement len_bytes
    is_at_bit7_rev: bool,
    /// Next byte to read from
    byte_idx: usize,
    /// Next bit to read from, starts from 7
    bit_idx: u8,
    /// First error of the `read_*_latch` methods since the last [latched](Self::latched)
    latch: Option<Error>,
}

impl<'i> BufReader<'i> {
    /// Create a new BufReader from the provided slice.
    pub fn new(buf: &'i [u8]) -> Self {
        let len_bytes = buf.len();
        Self {
            buf,
            len_bytes,
            is_at_bit7_rev: false,
            byte_idx: 0,
            bit_idx: 7,
            latch: None,
        }
    }

    /// Read bool with alignment of 1 bit.
    #[inline(always)]
    pub fn read_bool(&mut self) -> Result<bool, Error> {
        Ok(self.read_bit()? != 0)
    }

    // The integer and bool reads go through read_bit and read_le32: one copy of the bounds checks, and a
    // Result<u32, Error> comes back in two registers on 32-bit CPUs, where Result<bool, Error> or Result<u8, Error>
    // would go through memory at every call (docs/serdes/code_size.md).
    #[inline(never)]
    fn read_bit(&mut self) -> Result<u32, Error> {
        self.kept()?;
        if self.bits_in_byte_left() == 0 {
            return Err(Error::OutOfBoundsReadBool);
        }
        let val = (self.buf[self.byte_idx] >> self.bit_idx) & 1;
        if self.bit_idx == 0 {
            self.bit_idx = 7;
            self.byte_idx += 1;
        } else {
            self.bit_idx -= 1;
        }
        Ok(val as u32)
    }

    /// Align to byte and read `len` (1 to 4) bytes as a Little Endian number.
    #[inline(never)]
    fn read_le32(&mut self, len: usize) -> Result<u32, Error> {
        self.kept()?;
        let Some(bytes) = self.take_bytes(len) else {
            return Err(if len == 1 {
                Error::OutOfBoundsReadU8
            } else {
                Error::OutOfBoundsReadRawSlice
            });
        };
        let mut val = 0;
        for byte in bytes.iter().rev() {
            val = (val << 8) | *byte as u32;
        }
        Ok(val)
    }

    #[deprecated]
    pub fn read_u4(&mut self) -> Result<u8, Error> {
        Ok(self.read_nib()?.value())
    }

    /// Align to nibble and read one [Nibble].
    /// Note that there is a method read_un8(4, val), that will also read 4 bits from the buffer,
    /// but will use an alignment of 1 bit instead.
    pub fn read_nib(&mut self) -> Result<Nibble, Error> {
        self.kept()?;
        self.align_nibble();
        if self.nibbles_in_byte_left() == 0 {
            return Err(Error::OutOfBoundsReadU4);
        }
        if self.bit_idx == 7 {
            self.bit_idx = 3;
            Ok(Nibble::new_masked(self.buf[self.byte_idx] >> 4))
        } else {
            let val = Nibble::new_masked(self.buf[self.byte_idx]);
            self.bit_idx = 7;
            self.byte_idx += 1;
            Ok(val)
        }
    }

    /// Align to nibble and read one nibble into 4 lower bits of u8.
    /// See also [read_nib](Self::read_nib).
    pub fn read_nib_value(&mut self) -> Result<u8, Error> {
        Ok(self.read_nib()?.value())
    }

    /// Read up to 8 bits (without alignment) into u8 number.
    #[inline]
    pub fn read_un8(&mut self, bit_count: u8) -> Result<u8, Error> {
        if bit_count > 8 {
            return Err(Error::InvalidBitCount);
        }
        // Forwarded to read_un32 to only have one copy of the bit loop in flash, u32 ops are native on 32-bit MCUs.
        Ok(self.read_un32(bit_count)? as u8)
    }

    /// Read up to 16 bits (without alignment) into u16 number.
    #[inline]
    pub fn read_un16(&mut self, bit_count: u8) -> Result<u16, Error> {
        if bit_count > 16 {
            return Err(Error::InvalidBitCount);
        }
        Ok(self.read_un32(bit_count)? as u16)
    }

    read_unx!(read_un32, u32, 32);
    read_unx!(read_un64, u64, 64);

    /// Read bool like [read_bool](Self::read_bool), but an error is kept in the reader instead of returned
    /// (the value is then `false`): see [latched](Self::latched).
    #[inline(never)]
    pub fn read_bool_latch(&mut self) -> bool {
        match self.read_bit() {
            Ok(bit) => bit != 0,
            Err(e) => {
                self.latch.get_or_insert(e);
                false
            }
        }
    }

    #[inline(never)]
    fn read_le32_latch(&mut self, len: usize) -> u32 {
        match self.read_le32(len) {
            Ok(val) => val,
            Err(e) => {
                self.latch.get_or_insert(e);
                0
            }
        }
    }

    /// An error kept by a `_latch` read: while it is there no read moves, a read with a `Result` returns it and a
    /// `_latch` read gives 0 (SW-41). So a value whose plain fields are latched reports the first error even when
    /// a later field, read with `?`, fails too, and nothing read after the error decides anything (a discriminant
    /// is 0, a flag `false`). Not cleared: [latched](Self::latched) still returns it.
    #[inline(always)]
    fn kept(&self) -> Result<(), Error> {
        match self.latch {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// The first error of the `read_*_latch` methods since the last call, if any.
    ///
    /// The `_latch` reads are what `#[derive_shrink_wrap]` generates for plain fields: a run of them is followed
    /// by one `rd.latched()?` instead of an error check after each read, which is about half the code on small
    /// CPUs (docs/serdes/code_size.md). A read that fails gives 0, so do the `_latch` reads after it, and every
    /// read with a `Result` returns the kept error until it is taken here: don't use the values before
    /// `latched()` returned `Ok`, in particular not as a loop bound.
    #[inline]
    pub fn latched(&mut self) -> Result<(), Error> {
        match self.latch.take() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Read u8 with alignment of 1 byte.
    #[inline(always)]
    pub fn read_u8(&mut self) -> Result<u8, Error> {
        Ok(self.read_le32(1)? as u8)
    }

    /// Align to byte and read u16 in Little Endian.
    #[inline(always)]
    pub fn read_u16(&mut self) -> Result<u16, Error> {
        Ok(self.read_le32(2)? as u16)
    }

    /// Align to nibble and read a number encoded with UNib32 forward encoding, from the front of the buffer.
    pub fn read_unib32(&mut self) -> Result<u32, Error> {
        let value = UNib32::read_forward(self)?.0;

        Ok(value)
    }

    /// Align to byte and read a number encoded with UVlq32 encoding (big endian VLQ).
    pub fn read_uvlq32(&mut self) -> Result<u32, Error> {
        let value = UVlq32::read_forward(self)?.0;

        Ok(value)
    }

    /// Align to byte and read a number encoded with UVlq64 encoding (big endian VLQ).
    pub fn read_uvlq64(&mut self) -> Result<u64, Error> {
        let value = UVlq64::read_forward(self)?.0;

        Ok(value)
    }

    /// Read a number encoded with UNib32 backward encoding, from the back of the buffer.
    pub fn read_rev_len(&mut self) -> Result<usize, Error> {
        let value = UNib32::read_reversed(self)?.0;

        Ok(value as usize)
    }

    /// Align to byte and read u32 in Little Endian.
    #[inline(always)]
    pub fn read_u32(&mut self) -> Result<u32, Error> {
        self.read_le32(4)
    }

    /// Align to byte and read u64 in Little Endian.
    pub fn read_u64(&mut self) -> Result<u64, Error> {
        let u64_bytes: [u8; 8] = self
            .read_raw_slice(8)?
            .try_into()
            .map_err(|_| Error::InternalSliceToArrayCast)?;
        Ok(u64::from_le_bytes(u64_bytes))
    }

    /// Align to byte and read u128 in Little Endian.
    pub fn read_u128(&mut self) -> Result<u128, Error> {
        let u128_bytes: [u8; 16] = self
            .read_raw_slice(16)?
            .try_into()
            .map_err(|_| Error::InternalSliceToArrayCast)?;
        Ok(u128::from_le_bytes(u128_bytes))
    }

    /// Read i8 with alignment of 1 byte.
    pub fn read_i8(&mut self) -> Result<i8, Error> {
        Ok(self.read_u8()? as i8)
    }

    /// Align to byte and read i16 in Little Endian.
    #[inline(always)]
    pub fn read_i16(&mut self) -> Result<i16, Error> {
        Ok(self.read_le32(2)? as i16)
    }

    /// Align to byte and read i32 in Little Endian.
    #[inline(always)]
    pub fn read_i32(&mut self) -> Result<i32, Error> {
        Ok(self.read_le32(4)? as i32)
    }

    /// Align to byte and read i64 in Little Endian.
    pub fn read_i64(&mut self) -> Result<i64, Error> {
        let i64_bytes: [u8; 8] = self
            .read_raw_slice(8)?
            .try_into()
            .map_err(|_| Error::InternalSliceToArrayCast)?;
        Ok(i64::from_le_bytes(i64_bytes))
    }

    /// Align to byte and read i128 in Little Endian.
    pub fn read_i128(&mut self) -> Result<i128, Error> {
        let i128_bytes: [u8; 16] = self
            .read_raw_slice(16)?
            .try_into()
            .map_err(|_| Error::InternalSliceToArrayCast)?;
        Ok(i128::from_le_bytes(i128_bytes))
    }

    /// Align to byte and read u32 in Little Endian, then create f32 from it.
    #[inline(always)]
    pub fn read_f32(&mut self) -> Result<f32, Error> {
        Ok(f32::from_bits(self.read_le32(4)?))
    }

    /// Align to byte and read u64 in Little Endian, then create f64 from it.
    pub fn read_f64(&mut self) -> Result<f64, Error> {
        let f64_bytes: [u8; 8] = self
            .read_raw_slice(8)?
            .try_into()
            .map_err(|_| Error::InternalSliceToArrayCast)?;
        Ok(f64::from_le_bytes(f64_bytes))
    }

    /// [read_u8](Self::read_u8), an error is kept for [latched](Self::latched) and gives 0.
    #[inline(always)]
    pub fn read_u8_latch(&mut self) -> u8 {
        self.read_le32_latch(1) as u8
    }

    /// [read_u16](Self::read_u16), an error is kept for [latched](Self::latched) and gives 0.
    #[inline(always)]
    pub fn read_u16_latch(&mut self) -> u16 {
        self.read_le32_latch(2) as u16
    }

    /// [read_u32](Self::read_u32), an error is kept for [latched](Self::latched) and gives 0.
    #[inline(always)]
    pub fn read_u32_latch(&mut self) -> u32 {
        self.read_le32_latch(4)
    }

    /// [read_i8](Self::read_i8), an error is kept for [latched](Self::latched) and gives 0.
    #[inline(always)]
    pub fn read_i8_latch(&mut self) -> i8 {
        self.read_le32_latch(1) as i8
    }

    /// [read_i16](Self::read_i16), an error is kept for [latched](Self::latched) and gives 0.
    #[inline(always)]
    pub fn read_i16_latch(&mut self) -> i16 {
        self.read_le32_latch(2) as i16
    }

    /// [read_i32](Self::read_i32), an error is kept for [latched](Self::latched) and gives 0.
    #[inline(always)]
    pub fn read_i32_latch(&mut self) -> i32 {
        self.read_le32_latch(4) as i32
    }

    /// [read_f32](Self::read_f32), an error is kept for [latched](Self::latched) and gives 0.
    #[inline(always)]
    pub fn read_f32_latch(&mut self) -> f32 {
        f32::from_bits(self.read_le32_latch(4))
    }

    // The reads below are the plain ones with the error kept in the reader: the same function is called, so the
    // value, the error and the reader state are exactly those of the read with a `Result`
    // (shrink_wrap/tests/latched_reads.rs holds them against each other). One copy per kind of read.

    /// [read_un32](Self::read_un32), an error is kept for [latched](Self::latched) and gives 0.
    #[inline(never)]
    pub fn read_un32_latch(&mut self, bit_count: u8) -> u32 {
        match self.read_un32(bit_count) {
            Ok(val) => val,
            Err(e) => {
                self.latch.get_or_insert(e);
                0
            }
        }
    }

    /// [read_un8](Self::read_un8), an error is kept for [latched](Self::latched) and gives 0.
    #[inline(always)]
    pub fn read_un8_latch(&mut self, bit_count: u8) -> u8 {
        if bit_count > 8 {
            self.latch.get_or_insert(Error::InvalidBitCount);
            return 0;
        }
        self.read_un32_latch(bit_count) as u8
    }

    /// [read_un16](Self::read_un16), an error is kept for [latched](Self::latched) and gives 0.
    #[inline(always)]
    pub fn read_un16_latch(&mut self, bit_count: u8) -> u16 {
        if bit_count > 16 {
            self.latch.get_or_insert(Error::InvalidBitCount);
            return 0;
        }
        self.read_un32_latch(bit_count) as u16
    }

    /// [read_un64](Self::read_un64), an error is kept for [latched](Self::latched) and gives 0.
    #[inline]
    pub fn read_un64_latch(&mut self, bit_count: u8) -> u64 {
        match self.read_un64(bit_count) {
            Ok(val) => val,
            Err(e) => {
                self.latch.get_or_insert(e);
                0
            }
        }
    }

    /// What `#[derive_shrink_wrap]` reads a field of a user type with: [read](Self::read), except for the types
    /// that keep a failed read for [latched](Self::latched) (`U1`..`U64`, `I2`..`I64`), see
    /// [DeserializeShrinkWrap::des_shrink_wrap_latch].
    #[doc(hidden)]
    #[inline(always)]
    pub fn read_field<T: DeserializeShrinkWrap<'i>>(&mut self) -> Result<T, Error> {
        T::des_shrink_wrap_latch(self)
    }

    /// [read_field](Self::read_field) for [DeserializeShrinkWrapOwned] types.
    #[doc(hidden)]
    #[inline(always)]
    pub fn read_field_owned<T: DeserializeShrinkWrapOwned>(&mut self) -> Result<T, Error> {
        T::des_shrink_wrap_owned_latch(self)
    }

    /// Align to byte and create a slice with the provided length.
    pub fn read_raw_slice(&mut self, len: usize) -> Result<&'i [u8], Error> {
        self.kept()?;
        self.take_bytes(len).ok_or(Error::OutOfBoundsReadRawSlice)
    }

    /// [read_raw_slice](Self::read_raw_slice) without looking at a kept error
    #[inline(never)]
    fn take_bytes(&mut self, len: usize) -> Option<&'i [u8]> {
        self.align_byte();
        if self.bytes_left() < len {
            return None;
        }
        let val = &self.buf[self.byte_idx..self.byte_idx + len];
        self.byte_idx += len;
        Some(val)
    }

    /// Read length of the u8 slice from the back of the buffer and then the slice itself
    pub fn read_bytes(&mut self) -> Result<&'i [u8], Error> {
        let len_bytes = self.read_rev_len()?;
        self.read_raw_slice(len_bytes)
    }

    /// Read length of the string from the back of the buffer and then the string itself
    pub fn read_str(&mut self) -> Result<&'i str, Error> {
        let len_bytes = self.read_rev_len()?;
        let str_bytes = self.read_raw_slice(len_bytes)?;
        core::str::from_utf8(str_bytes).map_err(|_| Error::MalformedUtf8)
    }

    /// Read any value that implements DeserializeShrinkWrap.
    ///
    /// For Unsized types, size is read from the back of the buffer as reverse UNib32.
    /// Then [split](BufReader::split) reader is used to actually deserialize the value.
    ///
    /// Note that values deserialized with this method must be serialized with [write](crate::BufWriter::write).
    /// Values serialized with [ser_shrink_wrap](crate::SerializeShrinkWrap::ser_shrink_wrap) must be
    /// deserialized with [des_shrink_wrap](DeserializeShrinkWrap::des_shrink_wrap).
    pub fn read<T: DeserializeShrinkWrap<'i>>(&mut self) -> Result<T, Error> {
        if matches!(T::ELEMENT_SIZE, ElementSize::Unsized) {
            let size = self.read_rev_len()?;
            let mut rd_split = self.split(size)?;
            T::des_shrink_wrap(&mut rd_split)
        } else {
            T::des_shrink_wrap(self)
        }
    }

    /// Read any value that implements DeserializeShrinkWrapOwned.
    /// See [read](Self::read) for more information.
    pub fn read_owned<T: DeserializeShrinkWrapOwned>(&mut self) -> Result<T, Error> {
        if matches!(T::ELEMENT_SIZE, ElementSize::Unsized) {
            let size = self.read_rev_len()?;
            let mut rd_split = self.split(size)?;
            T::des_shrink_wrap_owned(&mut rd_split)
        } else {
            T::des_shrink_wrap_owned(self)
        }
    }

    /// Align to byte and split off a BufReader which can read up to the len bytes.
    /// This is the main mechanism used for forwards and backwards compatibility, as it allows for older code
    /// to ignore newer data by simply skipping additional bytes it doesn't know about.
    pub fn split(&mut self, len: usize) -> Result<Self, Error> {
        self.kept()?;
        self.align_byte();
        if self.bytes_left() < len {
            return Err(Error::OutOfBoundsSplit);
        }
        let prev_byte_idx = self.byte_idx;
        self.byte_idx += len;
        let buf = &self.buf[prev_byte_idx..prev_byte_idx + len];

        Ok(BufReader {
            buf,
            len_bytes: len,
            is_at_bit7_rev: false,
            byte_idx: 0,
            bit_idx: 7,
            latch: None,
        })
    }

    pub(crate) fn read_u4_rev(&mut self) -> Result<u8, Error> {
        self.kept()?;
        if self.byte_idx >= self.len_bytes {
            return Err(OutOfBoundsRev);
        }
        // the forward side is in the last byte: the nibble must be one it has not read into (SW-40), the low one
        // while it is at bit 3 or before, the high one only if it has read nothing of the byte
        if self.byte_idx + 1 == self.len_bytes {
            let free = if self.is_at_bit7_rev {
                self.bit_idx == 7
            } else {
                self.bit_idx >= 3
            };
            if !free {
                return Err(OutOfBoundsRev);
            }
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

    /// Align to byte.
    pub fn align_byte(&mut self) {
        if self.bit_idx == 7 {
            return;
        }
        self.bit_idx = 7;
        self.byte_idx += 1;
    }

    /// Returns the number of bytes left, taking into account that buffer is read from both sides.
    pub fn bytes_left(&self) -> usize {
        if self.byte_idx >= self.len_bytes {
            return 0;
        }
        let left = if self.bit_idx == 7 {
            self.len_bytes - self.byte_idx
        } else {
            // already read one or more bit or nibble from this byte, do not count it
            self.len_bytes - self.byte_idx - 1
        };
        if left == 0 {
            return 0;
        }
        if self.is_at_bit7_rev {
            // already read one nibble from the back, do not count this byte
            left - 1
        } else {
            left
        }
    }

    /// Returns the number of nibbles left, taking into account that buffer is read from both sides.
    pub fn nibbles_left(&self) -> usize {
        self.bytes_left() * 2 + self.nibbles_in_byte_left() as usize
    }

    fn nibbles_in_byte_left(&self) -> u8 {
        if self.byte_idx >= self.len_bytes {
            0
        } else {
            let is_last_byte = self.byte_idx + 1 == self.len_bytes;
            match (is_last_byte, self.bit_idx != 7, self.is_at_bit7_rev) {
                (true, false, false) => 2,
                (true, false, true) => 1,
                (true, true, false) => 1,
                (true, true, true) => 0,
                (false, _, _) if self.bit_idx == 7 => 2,
                (false, _, _) if self.bit_idx == 3 => 1,
                _ => 0,
            }
        }
    }

    /// Returns the number of bits left, taking into account that buffer is read from both sides.
    pub fn bits_left(&self) -> usize {
        self.bytes_left() * 8 + self.bits_in_byte_left() as usize
    }

    fn bits_in_byte_left(&self) -> u8 {
        if self.byte_idx >= self.len_bytes {
            return 0;
        }
        if self.byte_idx + 1 == self.len_bytes && self.is_at_bit7_rev {
            if self.bit_idx >= 3 {
                self.bit_idx + 1 - 4
            } else {
                0
            }
        } else {
            self.bit_idx + 1
        }
    }

    /// Return byte and bit position, that will be used on the next read call.
    pub fn pos(&self) -> (usize, u8) {
        (self.byte_idx, self.bit_idx)
    }
}

#[cfg(test)]
mod tests {
    use crate::{BufReader, BufWriter, Error};
    use hex_literal::hex;

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
    fn bits() {
        let mut rd = BufReader::new(&[0b0101_1100, 0b1010_0011]);
        assert_eq!(rd.read_bool(), Ok(false));
        assert_eq!(rd.read_bool(), Ok(true));
        assert_eq!(rd.read_bool(), Ok(false));
        assert_eq!(rd.read_bool(), Ok(true));
        assert_eq!(rd.read_bool(), Ok(true));
        assert_eq!(rd.read_bool(), Ok(true));
        assert_eq!(rd.read_bool(), Ok(false));
        assert_eq!(rd.read_bool(), Ok(false));

        assert_eq!(rd.read_bool(), Ok(true));
        assert_eq!(rd.read_bool(), Ok(false));
        assert_eq!(rd.read_bool(), Ok(true));
        assert_eq!(rd.read_bool(), Ok(false));
        assert_eq!(rd.read_bool(), Ok(false));
        assert_eq!(rd.read_bool(), Ok(false));
        assert_eq!(rd.read_bool(), Ok(true));
        assert_eq!(rd.read_bool(), Ok(true));

        assert_eq!(rd.bits_left(), 0);
    }

    #[test]
    fn floats() {
        let buf = hex!("0000803E");
        let mut rd = BufReader::new(&buf);
        assert_eq!(rd.read_f32(), Ok(0.25));
        assert_eq!(rd.bytes_left(), 0);

        let buf = hex!("000000000000D03F");
        let mut rd = BufReader::new(&buf);
        assert_eq!(rd.read_f64(), Ok(0.25));
        assert_eq!(rd.bytes_left(), 0);
    }

    #[test]
    fn read_u4_rev() {
        let buf = [0x43, 0x21];
        let mut rd = BufReader::new(&buf);
        assert_eq!(rd.bytes_left(), 2);

        assert!(!rd.is_at_bit7_rev);
        let value = rd.read_u4_rev().unwrap();
        assert_eq!(value, 1);
        assert_eq!(rd.len_bytes, 2);
        assert!(rd.is_at_bit7_rev);
        assert_eq!(rd.bytes_left(), 1);

        let value = rd.read_u4_rev().unwrap();
        assert_eq!(value, 2);
        assert_eq!(rd.len_bytes, 1);
        assert!(!rd.is_at_bit7_rev);
        assert_eq!(rd.bytes_left(), 1);

        let value = rd.read_u4_rev().unwrap();
        assert_eq!(value, 3);
        assert_eq!(rd.len_bytes, 1);
        assert!(rd.is_at_bit7_rev);
        assert_eq!(rd.bytes_left(), 0);

        let value = rd.read_u4_rev().unwrap();
        assert_eq!(value, 4);
        assert_eq!(rd.len_bytes, 0);
        assert!(!rd.is_at_bit7_rev);
        assert_eq!(rd.bytes_left(), 0);
    }

    #[test]
    fn nib16_rev() {
        let buf = [0xaa, 0x12, 0x09, 0x35];
        let mut rd = BufReader::new(&buf);
        assert_eq!(rd.bytes_left(), 4);

        let value = rd.read_rev_len().unwrap();
        assert_eq!(value, 5);

        assert_eq!(rd.read_u8().unwrap(), 0xaa);

        let value = rd.read_rev_len().unwrap();
        assert_eq!(value, 3);

        assert_eq!(rd.read_u8().unwrap(), 0x12);

        let value = rd.read_rev_len().unwrap();
        assert_eq!(value, 8);
        assert_eq!(rd.bytes_left(), 0);
    }

    #[test]
    fn u4_rev_overlap() {
        let buf = [0x10, 0x81];
        let mut rd = BufReader::new(&buf);
        let n = rd.read_rev_len().unwrap();
        assert_eq!(n, 1);
        let mut rd_split = rd.split(n).unwrap();
        let byte = rd_split.read_u8().unwrap();
        assert_eq!(byte, 0x10);
        let b = rd.read_bool().unwrap();
        assert!(b);
    }

    #[test]
    fn u4_rev_overlap_nib() {
        let buf = [0x10, 0xA1];
        let mut rd = BufReader::new(&buf);
        let n = rd.read_rev_len().unwrap();
        assert_eq!(n, 1);
        let mut rd_split = rd.split(n).unwrap();
        let byte = rd_split.read_u8().unwrap();
        assert_eq!(byte, 0x10);
        assert_eq!(rd.nibbles_left(), 1);
        let nib = rd.read_nib().unwrap();
        assert_eq!(nib.value(), 0xA);
    }

    #[test]
    fn read_un_full_width_aligned() {
        let buf = [0xAB, 0xCD, 0xEF];
        let mut rd = BufReader::new(&buf);
        assert_eq!(rd.read_un8(8), Ok(0xAB));
        assert_eq!(rd.read_un16(16), Ok(0xCDEF));
        assert_eq!(rd.read_un8(9), Err(Error::InvalidBitCount));
        assert_eq!(rd.read_un16(17), Err(Error::InvalidBitCount));
    }

    #[test]
    fn un_rev_overlap() {
        let buf = hex!("20 52 B9 6C 03");
        let mut rd = BufReader::new(&buf);
        assert_eq!(rd.read_rev_len(), Ok(3));
        assert_eq!(rd.read_un8(3), Ok(1));
        assert_eq!(rd.read_bool(), Ok(false));
        assert_eq!(rd.read_unib32(), Ok(0));
        assert_eq!(rd.read_un8(4), Ok(5));
        assert_eq!(rd.read_un8(5), Ok(5));
        assert_eq!(rd.read_un32(17), Ok(58_800));
        assert_eq!(rd.read_bool(), Ok(false));
    }

    #[test]
    fn sanity_left() {
        let buf = [1, 2, 3];
        let rd_seed = BufReader::new(&buf);

        // read_u4_rev after read_u4
        let mut rd = rd_seed;
        rd.read_u16().unwrap();
        rd.read_nib().unwrap();
        assert_eq!(rd.nibbles_left(), 1);
        assert_eq!(rd.nibbles_in_byte_left(), 1);
        assert_eq!(rd.bits_in_byte_left(), 4);
        rd.read_u4_rev().unwrap();
        assert_eq!(rd.nibbles_left(), 0);
        assert_eq!(rd.nibbles_in_byte_left(), 0);
        assert_eq!(rd.bits_in_byte_left(), 0);

        // read_u4_rev after read u1
        let mut rd = rd_seed;
        rd.read_u16().unwrap();
        rd.read_un8(1).unwrap();
        assert_eq!(rd.bits_in_byte_left(), 7);
        assert_eq!(rd.nibbles_left(), 1);
        assert_eq!(rd.nibbles_in_byte_left(), 1);
        rd.read_u4_rev().unwrap();
        assert_eq!(rd.nibbles_left(), 0);
        assert_eq!(rd.nibbles_in_byte_left(), 0);
        assert_eq!(rd.bits_in_byte_left(), 3);

        // read_bool meets read_u4_rev
        let mut rd = rd_seed;
        rd.read_u16().unwrap();
        rd.read_nib().unwrap();
        rd.read_u4_rev().unwrap();
        assert!(rd.read_bool().is_err());

        // read_u8 meets read_u4_rev
        let mut rd = rd_seed;
        rd.read_u16().unwrap();
        rd.read_u4_rev().unwrap();
        assert!(rd.read_u8().is_err());
    }

    #[test]
    fn integers() {
        let buf = hex!("12 1234 12345678 123456789ABCDEF0 12003411562278339A44BC55DE66F077");
        let mut rd = BufReader::new(&buf);
        assert_eq!(rd.read_u8().unwrap(), 0x12);
        assert_eq!(rd.read_u16().unwrap(), 0x3412);
        assert_eq!(rd.read_u32().unwrap(), 0x78563412);
        assert_eq!(rd.read_u64().unwrap(), 0xF0DEBC9A78563412);
        assert_eq!(rd.read_u128().unwrap(), 0x77F066DE55BC449A3378225611340012);

        let buf = hex!("88 5CFE 0000F0FF 7B00000000000080 00010000000000000000000000000080");
        let mut rd = BufReader::new(&buf);
        assert_eq!(rd.read_i8(), Ok(-120));
        assert_eq!(rd.read_i16(), Ok(-420));
        assert_eq!(rd.read_i32(), Ok(-1_048_576));
        assert_eq!(rd.read_i64(), Ok(i64::MIN + 123));
        assert_eq!(rd.read_i128(), Ok(i128::MIN + 256));
    }

    #[test]
    fn read_bool_out_of_bounds() {
        let mut rd = BufReader::new(&[]);
        assert_eq!(rd.read_bool(), Err(Error::OutOfBoundsReadBool));
    }

    #[test]
    #[allow(deprecated)]
    fn nib_reads_and_out_of_bounds() {
        let buf = [0xAB];
        let mut rd = BufReader::new(&buf);
        assert_eq!(rd.read_nib_value(), Ok(0xA));
        assert_eq!(rd.read_u4(), Ok(0xB));
        // both nibbles consumed, nothing left to read
        assert_eq!(rd.read_nib(), Err(Error::OutOfBoundsReadU4));
    }

    #[test]
    fn align_nibble_mid_byte() {
        // bit_idx ends up at 5 (> 3), align_nibble() should snap it to 3
        // without advancing to the next byte.
        let buf = [0b1111_0101];
        let mut rd = BufReader::new(&buf);
        assert_eq!(rd.read_bool(), Ok(true));
        assert_eq!(rd.read_bool(), Ok(true));
        assert_eq!(rd.read_nib_value(), Ok(0b0101));
    }

    #[test]
    fn latched_reads() {
        let buf = [0x80, 0x34, 0x12, 0xAA];
        let mut rd = BufReader::new(&buf);
        assert!(rd.read_bool_latch());
        assert_eq!(rd.read_u16_latch(), 0x1234);
        assert_eq!(rd.latched(), Ok(()));
        // 4 bytes wanted, 1 left: 0, the error is kept, and the reads after it give 0 without moving (SW-41)
        assert_eq!(rd.read_u32_latch(), 0);
        let pos = rd.pos();
        assert_eq!(rd.read_u8_latch(), 0);
        assert!(!rd.read_bool_latch());
        assert_eq!(rd.pos(), pos);
        // a read with a Result returns the kept error too, and keeps it
        assert_eq!(rd.read_u8(), Err(Error::OutOfBoundsReadRawSlice));
        assert_eq!(rd.read_bool(), Err(Error::OutOfBoundsReadRawSlice));
        assert_eq!(rd.read_nib(), Err(Error::OutOfBoundsReadRawSlice));
        assert_eq!(rd.read_un8(3), Err(Error::OutOfBoundsReadRawSlice));
        assert_eq!(rd.read_un64(3), Err(Error::OutOfBoundsReadRawSlice));
        assert_eq!(rd.read_raw_slice(1), Err(Error::OutOfBoundsReadRawSlice));
        assert_eq!(rd.split(1), Err(Error::OutOfBoundsReadRawSlice));
        assert_eq!(rd.read_rev_len(), Err(Error::OutOfBoundsReadRawSlice));
        assert_eq!(rd.pos(), pos);
        // the first error, once
        assert_eq!(rd.latched(), Err(Error::OutOfBoundsReadRawSlice));
        assert_eq!(rd.latched(), Ok(()));
        assert_eq!(rd.read_u8_latch(), 0xAA);
    }

    #[test]
    fn read_u8_out_of_bounds() {
        let mut rd = BufReader::new(&[]);
        assert_eq!(rd.read_u8(), Err(Error::OutOfBoundsReadU8));
    }

    #[test]
    fn read_raw_slice_out_of_bounds() {
        let buf = [1, 2];
        let mut rd = BufReader::new(&buf);
        assert_eq!(rd.read_raw_slice(3), Err(Error::OutOfBoundsReadRawSlice));
    }

    #[test]
    fn split_out_of_bounds() {
        let buf = [1, 2];
        let mut rd = BufReader::new(&buf);
        assert_eq!(rd.split(3), Err(Error::OutOfBoundsSplit));
    }

    #[test]
    fn bytes_var_len() {
        let mut buf = [0u8; 8];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_bytes(&[1, 2, 3]).unwrap();
        let buf = wr.finish().unwrap();

        let mut rd = BufReader::new(buf);
        assert_eq!(rd.read_bytes(), Ok(&[1u8, 2, 3][..]));
    }

    #[test]
    fn str_round_trip_and_malformed_utf8() {
        let mut buf = [0u8; 8];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_str("hi").unwrap();
        let buf = wr.finish().unwrap();
        let mut rd = BufReader::new(buf);
        assert_eq!(rd.read_str(), Ok("hi"));

        let mut buf = [0u8; 8];
        let mut wr = BufWriter::new(&mut buf);
        // 0xFF is not valid UTF-8 on its own
        wr.write_bytes(&[0xFF]).unwrap();
        let buf = wr.finish().unwrap();
        let mut rd = BufReader::new(buf);
        assert_eq!(rd.read_str(), Err(Error::MalformedUtf8));
    }

    #[test]
    fn read_generic_sized_and_unsized() {
        let mut buf = [0u8; 16];
        let mut wr = BufWriter::new(&mut buf);
        wr.write(&true).unwrap();
        wr.write(&Box::new(42u32)).unwrap();
        let buf = wr.finish().unwrap();

        let mut rd = BufReader::new(buf);
        assert_eq!(rd.read::<bool>(), Ok(true));
        assert_eq!(rd.read::<Box<u32>>(), Ok(Box::new(42)));
    }

    #[test]
    fn read_owned_generic_sized_and_unsized() {
        let mut buf = [0u8; 16];
        let mut wr = BufWriter::new(&mut buf);
        wr.write(&true).unwrap();
        wr.write(&Box::new(42u32)).unwrap();
        let buf = wr.finish().unwrap();

        let mut rd = BufReader::new(buf);
        assert_eq!(rd.read_owned::<bool>(), Ok(true));
        assert_eq!(rd.read_owned::<Box<u32>>(), Ok(Box::new(42)));
    }

    #[test]
    fn pos_tracks_progress() {
        let buf = [0xAB, 0xCD];
        let mut rd = BufReader::new(&buf);
        assert_eq!(rd.pos(), (0, 7));
        rd.read_nib().unwrap();
        assert_eq!(rd.pos(), (0, 3));
        rd.read_nib().unwrap();
        assert_eq!(rd.pos(), (1, 7));
    }

    #[test]
    fn nibbles_in_byte_left_all_cases() {
        let buf = [1, 2, 3];

        // (false, bit_idx == 7) -> 2: not the last byte, nothing read from it yet.
        let mut rd = BufReader::new(&buf);
        assert_eq!(rd.nibbles_in_byte_left(), 2);

        // (false, bit_idx == 3) -> 1: not the last byte, one nibble already read.
        rd.read_nib().unwrap();
        assert_eq!(rd.nibbles_in_byte_left(), 1);

        // (false, _) default -> 0: not the last byte, mid-bit position.
        let mut rd = BufReader::new(&buf);
        rd.read_bool().unwrap();
        rd.read_bool().unwrap();
        assert_eq!(rd.nibbles_in_byte_left(), 0);

        // (true, bit_idx == 7, is_at_bit7_rev == false) -> 2: last byte, untouched.
        let mut rd = BufReader::new(&buf);
        rd.read_u16().unwrap();
        assert_eq!(rd.nibbles_in_byte_left(), 2);

        // (true, bit_idx == 7, is_at_bit7_rev == true) -> 1.
        rd.read_u4_rev().unwrap();
        assert_eq!(rd.nibbles_in_byte_left(), 1);
    }

    #[test]
    fn bits_in_byte_left_low_bit_idx() {
        // 5 bits of the last byte are read: its low nibble is no longer free for a length (SW-40), this used to
        // give one and leave the reader in the is_at_bit7_rev, bit_idx < 3 state, now unreachable.
        let buf = [1, 2, 3];
        let mut rd = BufReader::new(&buf);
        rd.read_u16().unwrap();
        rd.read_un8(5).unwrap();
        assert_eq!(rd.read_u4_rev(), Err(Error::OutOfBoundsRev));
        assert!(!rd.is_at_bit7_rev);
        assert_eq!(rd.bits_in_byte_left(), 3);
    }

    #[test]
    fn bytes_left_zero_mid_last_byte() {
        // left == 0 before the is_at_bit7_rev check is applied.
        let buf = [1];
        let mut rd = BufReader::new(&buf);
        rd.read_nib().unwrap();
        assert_eq!(rd.bytes_left(), 0);
    }

    #[test]
    fn read_u4_rev_out_of_bounds() {
        let buf = [0xAB];
        let mut rd = BufReader::new(&buf);
        assert_eq!(rd.read_u4_rev(), Ok(0xB));
        assert_eq!(rd.read_u4_rev(), Ok(0xA));
        // both the forward and reverse cursors now point at the same spot, nothing left
        assert_eq!(rd.read_u4_rev(), Err(Error::OutOfBoundsRev));
    }

    #[test]
    fn align_nibble_low_bits_advances_byte() {
        // bit_idx ends up at 2 (< 3), align_nibble() should advance to the next byte
        // and reset bit_idx to 7, instead of snapping within the current byte.
        let buf = [0xFF, 0xAB];
        let mut rd = BufReader::new(&buf);
        for _ in 0..5 {
            rd.read_bool().unwrap();
        }
        assert_eq!(rd.read_nib_value(), Ok(0xA));
    }

    /// A read of the model: what it takes and how it fails.
    #[derive(Debug, Clone, Copy)]
    enum Op {
        Bool,
        BoolLatch,
        Un8(u8),
        Un16(u8),
        Un32(u8),
        Un32Latch(u8),
        Un64(u8),
        Un64Latch(u8),
        Nib,
        U8,
        U8Latch,
        U16,
        U32,
        U64,
        Raw(usize),
        Rev,
    }

    impl Op {
        fn all() -> Vec<Op> {
            let mut ops = vec![Op::Bool, Op::BoolLatch, Op::Nib, Op::U8, Op::U8Latch];
            ops.extend([Op::U16, Op::U32, Op::U64, Op::Rev]);
            ops.extend((0..4).map(Op::Raw));
            ops.extend((1..=8).map(Op::Un8));
            ops.extend((1..=16).map(Op::Un16));
            ops.extend((1..=32).flat_map(|n| [Op::Un32(n), Op::Un32Latch(n)]));
            ops.extend((1..=64).flat_map(|n| [Op::Un64(n), Op::Un64Latch(n)]));
            ops
        }

        fn run(self, rd: &mut BufReader) -> Result<u64, Error> {
            let latched = |rd: &mut BufReader, val: u64| rd.latched().map(|_| val);
            match self {
                Op::Bool => rd.read_bool().map(u64::from),
                Op::BoolLatch => {
                    let val = rd.read_bool_latch();
                    latched(rd, val as u64)
                }
                Op::Un8(n) => rd.read_un8(n).map(u64::from),
                Op::Un16(n) => rd.read_un16(n).map(u64::from),
                Op::Un32(n) => rd.read_un32(n).map(u64::from),
                Op::Un32Latch(n) => {
                    let val = rd.read_un32_latch(n);
                    latched(rd, val as u64)
                }
                Op::Un64(n) => rd.read_un64(n),
                Op::Un64Latch(n) => {
                    let val = rd.read_un64_latch(n);
                    latched(rd, val)
                }
                Op::Nib => rd.read_nib().map(|nib| nib.value() as u64),
                Op::U8 => rd.read_u8().map(u64::from),
                Op::U8Latch => {
                    let val = rd.read_u8_latch();
                    latched(rd, val as u64)
                }
                Op::U16 => rd.read_u16().map(u64::from),
                Op::U32 => rd.read_u32().map(u64::from),
                Op::U64 => rd.read_u64(),
                Op::Raw(len) => rd
                    .read_raw_slice(len)
                    .map(|bytes| bytes.iter().rev().fold(0, |v, b| v << 8 | *b as u64)),
                Op::Rev => rd.read_u4_rev().map(u64::from),
            }
        }

        /// What the read must give from a reader whose forward side is at bit `fwd` and whose lengths read from
        /// the back begin at bit `bound` (bits counted from the start, MSB first), and where the forward side is
        /// afterwards.
        fn expect(self, buf: &[u8], fwd: usize, bound: usize) -> (Result<u64, Error>, usize) {
            let bits = |from: usize, n: usize| {
                (from..from + n).fold(0u64, |v, i| {
                    v << 1 | ((buf[i / 8] >> (7 - i % 8)) & 1) as u64
                })
            };
            // n bits from bit `at`, if they are all before the lengths
            let take = |at: usize, n: usize, err: Error| {
                if at + n <= bound {
                    (Ok(bits(at, n)), at + n)
                } else {
                    (Err(err), fwd)
                }
            };
            let bytes = |n: usize, err: Error| {
                let at = fwd.div_ceil(8) * 8;
                // whole bytes only: a byte whose low nibble is a length is not one; no bytes are always there
                if n == 0 || at + n * 8 <= bound / 8 * 8 {
                    let le = (0..n).rev().fold(0u64, |v, i| v << 8 | bits(at + i * 8, 8));
                    (Ok(le), at + n * 8)
                } else {
                    (Err(err), fwd)
                }
            };
            match self {
                Op::Bool | Op::BoolLatch => take(fwd, 1, Error::OutOfBoundsReadBool),
                Op::Un8(n)
                | Op::Un16(n)
                | Op::Un32(n)
                | Op::Un32Latch(n)
                | Op::Un64(n)
                | Op::Un64Latch(n) => take(fwd, n as usize, Error::OutOfBoundsReadUN),
                Op::Nib => take(fwd.div_ceil(4) * 4, 4, Error::OutOfBoundsReadU4),
                Op::U8 | Op::U8Latch => bytes(1, Error::OutOfBoundsReadU8),
                Op::U16 => bytes(2, Error::OutOfBoundsReadRawSlice),
                Op::U32 => bytes(4, Error::OutOfBoundsReadRawSlice),
                Op::U64 => bytes(8, Error::OutOfBoundsReadRawSlice),
                Op::Raw(len) => bytes(len, Error::OutOfBoundsReadRawSlice),
                Op::Rev => {
                    if bound >= 4 && fwd <= bound - 4 {
                        (Ok(bits(bound - 4, 4)), fwd)
                    } else {
                        (Err(Error::OutOfBoundsRev), fwd)
                    }
                }
            }
        }
    }

    /// The forward side's bit and where the lengths begin, see [Op::expect].
    fn sides(rd: &BufReader) -> (usize, usize) {
        let fwd = rd.byte_idx * 8 + (7 - rd.bit_idx) as usize;
        let bound = rd.len_bytes * 8 - if rd.is_at_bit7_rev { 4 } else { 0 };
        (fwd, bound)
    }

    /// Every reader state of buffers of 1 to 3 bytes (forward position, lengths read from the back), every
    /// read: a read never takes a bit of the other side, and fails only when its bits are not there (SW-40).
    #[test]
    fn bounds_every_state_every_read() {
        let mut states = 0;
        for buf in [
            &[0xA5u8][..],
            &[0x3C, 0xA1],
            &[0xFF, 0x00, 0x96],
            &[0x12, 0x34, 0x56],
        ] {
            for len_bytes in 0..=buf.len() {
                for is_at_bit7_rev in [false, true] {
                    if is_at_bit7_rev && len_bytes == 0 {
                        continue;
                    }
                    let bound = len_bytes * 8 - if is_at_bit7_rev { 4 } else { 0 };
                    for fwd in 0..=buf.len() * 8 {
                        let (byte_idx, bit_idx) = (fwd / 8, 7 - (fwd % 8) as u8);
                        // past the lengths only by aligning to the next byte, i.e. at a byte at or after them
                        if fwd > bound && !(bit_idx == 7 && byte_idx >= len_bytes) {
                            continue;
                        }
                        let rd = BufReader {
                            buf,
                            len_bytes,
                            is_at_bit7_rev,
                            byte_idx,
                            bit_idx,
                            latch: None,
                        };
                        states += 1;
                        for op in Op::all() {
                            let mut rd = rd;
                            let (expected, fwd_after) = op.expect(buf, fwd, bound);
                            let what =
                                format!("{op:?} on {buf:02x?} at {fwd}, lengths from {bound}");
                            assert_eq!(op.run(&mut rd), expected, "{what}");
                            if expected.is_ok() {
                                let bound_after = if matches!(op, Op::Rev) {
                                    bound - 4
                                } else {
                                    bound
                                };
                                assert_eq!(sides(&rd), (fwd_after, bound_after), "{what}");
                            }
                        }
                    }
                }
            }
        }
        assert!(states > 100, "{states}");
    }
}

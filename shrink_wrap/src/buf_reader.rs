use crate::nib32::UNib32;
use crate::un::read_unx;
use crate::Error::OutOfBoundsRev;
use crate::{DeserializeShrinkWrap, ElementSize, Error};

/// Buffer reader that treats input as a stream of bits, nibbles or bytes.
#[derive(Copy, Clone)]
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
        }
    }

    /// Read bool with alignment of 1 bit.
    pub fn read_bool(&mut self) -> Result<bool, Error> {
        if self.bits_in_byte_left() == 0 {
            return Err(Error::OutOfBoundsReadBool);
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

    /// Align to nibble and read one nibble into 4 lower bits of u8.
    /// Note that there is a method read_un8(4, val), that will also read 4 bits from the buffer,
    /// but will use an alignment of 1 bit instead.
    pub fn read_u4(&mut self) -> Result<u8, Error> {
        self.align_nibble();
        if (self.bytes_left() == 0) && self.bit_idx == 7 {
            return Err(Error::OutOfBoundsReadU4);
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

    read_unx!(read_un8, u8, 8);
    read_unx!(read_un16, u16, 16);
    read_unx!(read_un32, u32, 32);
    read_unx!(read_un64, u64, 64);

    /// Read u8 with alignment of 1 byte.
    pub fn read_u8(&mut self) -> Result<u8, Error> {
        self.align_byte();
        if self.bytes_left() == 0 {
            return Err(Error::OutOfBoundsReadU8);
        }
        let val = self.buf[self.byte_idx];
        self.byte_idx += 1;
        Ok(val)
    }

    /// Align to byte and read u16 in Little Endian.
    pub fn read_u16(&mut self) -> Result<u16, Error> {
        let u16_bytes: [u8; 2] = self
            .read_raw_slice(2)?
            .try_into()
            .map_err(|_| Error::InternalSliceToArrayCast)?;
        Ok(u16::from_le_bytes(u16_bytes))
    }

    /// Align to nibble and read a number encoded with UNib32 forward encoding, from the front of the buffer.
    pub fn read_unib32(&mut self) -> Result<u32, Error> {
        let value = UNib32::read_forward(self)?.0;

        #[cfg(feature = "defmt-extended")]
        defmt::trace!("read_unib32() = {}", value);
        #[cfg(feature = "tracing-extended")]
        tracing::trace!("read_unib32() = {}", value);

        Ok(value)
    }

    /// Read a number encoded with UNib32 backward encoding, from the back of the buffer.
    pub fn read_unib32_rev(&mut self) -> Result<u32, Error> {
        let value = UNib32::read_reversed(self)?.0;

        #[cfg(feature = "defmt-extended")]
        defmt::trace!("read_unib32_rev() = {}", value);
        #[cfg(feature = "tracing-extended")]
        tracing::trace!("read_unib32_rev() = {}", value);

        Ok(value)
    }

    /// Align to byte and read u32 in Little Endian.
    pub fn read_u32(&mut self) -> Result<u32, Error> {
        let u32_bytes: [u8; 4] = self
            .read_raw_slice(4)?
            .try_into()
            .map_err(|_| Error::InternalSliceToArrayCast)?;
        Ok(u32::from_le_bytes(u32_bytes))
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
    pub fn read_i16(&mut self) -> Result<i16, Error> {
        let i16_bytes: [u8; 2] = self
            .read_raw_slice(2)?
            .try_into()
            .map_err(|_| Error::InternalSliceToArrayCast)?;
        Ok(i16::from_le_bytes(i16_bytes))
    }

    /// Align to byte and read i32 in Little Endian.
    pub fn read_i32(&mut self) -> Result<i32, Error> {
        let i32_bytes: [u8; 4] = self
            .read_raw_slice(4)?
            .try_into()
            .map_err(|_| Error::InternalSliceToArrayCast)?;
        Ok(i32::from_le_bytes(i32_bytes))
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
    pub fn read_f32(&mut self) -> Result<f32, Error> {
        let f32_bytes: [u8; 4] = self
            .read_raw_slice(4)?
            .try_into()
            .map_err(|_| Error::InternalSliceToArrayCast)?;
        Ok(f32::from_le_bytes(f32_bytes))
    }

    /// Align to byte and read u64 in Little Endian, then create f64 from it.
    pub fn read_f64(&mut self) -> Result<f64, Error> {
        let f64_bytes: [u8; 8] = self
            .read_raw_slice(8)?
            .try_into()
            .map_err(|_| Error::InternalSliceToArrayCast)?;
        Ok(f64::from_le_bytes(f64_bytes))
    }

    /// Align to byte and create a slice with the provided length.
    pub fn read_raw_slice(&mut self, len: usize) -> Result<&'i [u8], Error> {
        self.align_byte();
        if self.bytes_left() < len {
            return Err(Error::OutOfBoundsReadRawSlice);
        }
        let val = &self.buf[self.byte_idx..self.byte_idx + len];
        self.byte_idx += len;
        Ok(val)
    }

    // pub fn read_bytes(&mut self) -> Result<&'i [u8], Error> {
    //     let len_bytes = self.read_unib32_rev()? as usize;
    //     self.read_raw_slice(len_bytes)
    // }

    /// Consume BufReader and treat all the remaining bytes as UTF8 encoded str.
    /// The intended way to use this method is to first read the length of the string using
    /// read_unib32_rev, then split the original BufReader and use this method.
    pub fn read_raw_str(mut self) -> Result<&'i str, Error> {
        let len_bytes = self.bytes_left();
        let str_bytes = self.read_raw_slice(len_bytes)?;
        core::str::from_utf8(str_bytes).map_err(|_| Error::MalformedUtf8)
    }

    /// Read any value that implements DeserializeShrinkWrap.
    ///
    /// For Unsized types, size is read from the back of the buffer as reverse UNib32.
    /// Then [split](BufReader::split) reader is used to actually deserialize the value.
    ///
    /// Note that values deserialized with this method must be serialized with [write](crate::BufWriter::write).
    /// Values serialized with [ser_shrink_wrap](SerializeShrinkWrap::ser_shrink_wrap) must be
    /// deserialized with [des_shrink_wrap](DeserializeShrinkWrap::des_shrink_wrap).
    pub fn read<T: DeserializeShrinkWrap<'i>>(&mut self) -> Result<T, Error> {
        if matches!(T::ELEMENT_SIZE, ElementSize::Unsized) {
            #[cfg(feature = "tracing-extended")]
            tracing::trace!("reading Unsized object");
            let size = self.read_unib32_rev()? as usize;
            let mut rd_split = self.split(size)?;
            T::des_shrink_wrap(&mut rd_split)
        } else {
            #[cfg(feature = "tracing-extended")]
            tracing::trace!("reading ?Sized object");
            T::des_shrink_wrap(self)
        }
    }

    /// Align to byte and split off a BufReader which can read up to the len bytes.
    /// This is the main mechanism used for forwards and backwards compatibility, as it allows for older code
    /// to ignore newer data by simply skipping additional bytes it doesn't know about.
    pub fn split(&mut self, len: usize) -> Result<Self, Error> {
        self.align_byte();
        if self.bytes_left() < len {
            return Err(Error::OutOfBoundsSplit(UNib32(len as u32)));
        }
        let prev_byte_idx = self.byte_idx;
        self.byte_idx += len;
        let buf = &self.buf[prev_byte_idx..prev_byte_idx + len];

        #[cfg(feature = "defmt-extended")]
        defmt::trace!(
            "split({}): prev_byte_idx={} {=[u8]:x}",
            len,
            prev_byte_idx,
            buf
        );
        #[cfg(feature = "tracing-extended")]
        tracing::trace!("split({len}): prev_byte_idx={prev_byte_idx} {:02x?}", buf);

        Ok(BufReader {
            buf,
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
            self.len_bytes - self.byte_idx - 1
        };
        if left == 0 {
            return 0;
        }
        if self.is_at_bit7_rev {
            left - 1
        } else {
            left
        }
    }

    /// Returns the number of nibbles left, taking into account that buffer is read from both sides.
    #[inline]
    pub fn nibbles_left(&self) -> usize {
        self.bytes_left() * 2 + if self.bit_idx == 3 { 1 } else { 0 }
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
    use crate::BufReader;
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
    fn float() {
        let buf = [0, 0, 0x80, 0x3E];
        let mut rd = BufReader::new(&buf);
        assert_eq!(rd.read_f32(), Ok(0.25));
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

        let value = rd.read_unib32_rev().unwrap();
        assert_eq!(value, 5);

        assert_eq!(rd.read_u8().unwrap(), 0xaa);

        let value = rd.read_unib32_rev().unwrap();
        assert_eq!(value, 3);

        assert_eq!(rd.read_u8().unwrap(), 0x12);

        let value = rd.read_unib32_rev().unwrap();
        assert_eq!(value, 8);
        assert_eq!(rd.bytes_left(), 0);
    }

    #[test]
    fn u4_rev_overlap() {
        let buf = [0x10, 0x81];
        let mut rd = BufReader::new(&buf);
        let n = rd.read_unib32_rev().unwrap();
        assert_eq!(n, 1);
        let mut rd_split = rd.split(n as usize).unwrap();
        let byte = rd_split.read_u8().unwrap();
        assert_eq!(byte, 0x10);
        let b = rd.read_bool().unwrap();
        assert!(b);
    }

    #[test]
    fn un_rev_overlap() {
        let buf = hex!("20 52 B9 6C 03");
        let mut rd = BufReader::new(&buf);
        assert_eq!(rd.read_unib32_rev(), Ok(3));
        assert_eq!(rd.read_un8(3), Ok(1));
        assert_eq!(rd.read_bool(), Ok(false));
        assert_eq!(rd.read_unib32(), Ok(0));
        assert_eq!(rd.read_un8(4), Ok(5));
        assert_eq!(rd.read_un8(5), Ok(5));
        assert_eq!(rd.read_un32(17), Ok(58_800));
        assert_eq!(rd.read_bool(), Ok(false));
    }
}

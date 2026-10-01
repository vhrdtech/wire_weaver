use core::fmt::{Debug, Display, Formatter};

use crate::{
    BufReader, BufWriter, DeserializeShrinkWrap, DeserializeShrinkWrapOwned, ElementSize, Error,
    SerializeShrinkWrap,
};

/// Variable length encoded u32 based on bytes (VLQ, big endian).
/// Each byte carries 1 bit (MSB) indicating whether there are more bytes + 7 bits from the original number,
/// most significant group first. Takes from 1 to 5 bytes, alignment of 1 byte is used.
///
/// Compared to [UNib32](crate::UNib32), takes more space for small numbers (0..=7 fit into one nibble),
/// but less for big ones, and is a well-known encoding (MIDI, ASN.1 OID).
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct UVlq32(pub u32);

const ONE_MORE_BYTE: u8 = 0b1000_0000;
const MAX_LEN: usize = 5;

impl UVlq32 {
    pub fn len_bytes(&self) -> usize {
        if self.0 == 0 {
            1
        } else {
            ((32 - self.0.leading_zeros()) as usize).div_ceil(7)
        }
    }

    /// Encode into the maximum length, right-justified: canonical encoding is in `bytes[start..]`, while `bytes`
    /// as a whole is the same number padded with empty `0x80` groups in front, which is also valid to read.
    pub(crate) fn encode_right_justified(&self) -> ([u8; MAX_LEN], usize) {
        let mut bytes = [ONE_MORE_BYTE; MAX_LEN];
        let len = self.len_bytes();
        for i in 0..len {
            let byte = ((self.0 >> (i * 7)) & 0x7f) as u8;
            bytes[MAX_LEN - 1 - i] = if i > 0 { byte | ONE_MORE_BYTE } else { byte };
        }
        (bytes, MAX_LEN - len)
    }

    pub(crate) fn write_forward(&self, wr: &mut BufWriter) -> Result<(), Error> {
        let (bytes, start) = self.encode_right_justified();
        wr.write_raw_slice(&bytes[start..])
    }

    pub(crate) fn read_forward(rd: &mut BufReader) -> Result<Self, Error> {
        let mut num: u32 = 0;
        for _ in 0..MAX_LEN {
            let byte = rd.read_u8()?;
            if num > (u32::MAX >> 7) {
                // shifting would lose the most significant bits
                return Err(Error::MalformedUVlq32);
            }
            num = (num << 7) | (byte & 0x7f) as u32;
            if byte & ONE_MORE_BYTE == 0 {
                return Ok(UVlq32(num));
            }
        }
        // 5th byte should be the last for u32
        Err(Error::MalformedUVlq32)
    }
}

impl SerializeShrinkWrap for UVlq32 {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        self.write_forward(wr)
    }
}

#[cfg(feature = "std")]
impl crate::SerializeShrinkWrapOwned for UVlq32 {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn ser_shrink_wrap_owned(&self, wr: &mut crate::BufWriterOwned) -> Result<(), Error> {
        wr.write_uvlq32(self.0)
    }
}

impl<'i> DeserializeShrinkWrap<'i> for UVlq32 {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        UVlq32::read_forward(rd)
    }
}

impl DeserializeShrinkWrapOwned for UVlq32 {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, Error> {
        UVlq32::read_forward(rd)
    }
}

impl From<UVlq32> for u32 {
    fn from(value: UVlq32) -> Self {
        value.0
    }
}

impl From<u32> for UVlq32 {
    fn from(num: u32) -> Self {
        UVlq32(num)
    }
}

impl Debug for UVlq32 {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Display for UVlq32 {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// [UVlq32] that is always written as the maximum 5 bytes, so that its value can be filled in after the rest of the
/// buffer is serialized, and the unused leading bytes are then dropped by taking a shorter slice.
///
/// Meant for the first field of a message, whose value is only known when the message is about to be sent
/// (e.g. request sequence number), while the message itself is serialized in advance:
/// 1. Serialize the message with `UVlq32Backfill(0)` (or any other value) as its first field.
/// 2. Call [UVlq32Backfill::backfill] on the serialized bytes, and send the slice it returns.
///
/// On the wire it is a [UVlq32], either in canonical (shortest) form if the slice returned by
/// [backfill](UVlq32Backfill::backfill) is used, or padded with empty `0x80` groups in front if the whole buffer is
/// used. Both are valid and read back as the same number, by both `UVlq32` and `UVlq32Backfill`.
///
/// `#[derive_shrink_wrap(..)]` rejects it anywhere other than the first field of a struct (also inside `Option`,
/// `Vec`, tuples, arrays and enums), but can't check that the struct itself is serialized at the start of the buffer.
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct UVlq32Backfill(pub u32);

impl UVlq32Backfill {
    /// Number of bytes always reserved for the value when serializing.
    pub const LEN: usize = MAX_LEN;

    /// Write `value` into the first [LEN](Self::LEN) bytes of `bytes`, previously serialized with `UVlq32Backfill`
    /// as the first field, and return the slice starting at its canonical encoding, to the end of `bytes`.
    ///
    /// The skipped leading bytes are set to the padded form, so `bytes` as a whole stays valid too.
    pub fn backfill(bytes: &mut [u8], value: u32) -> Result<&mut [u8], Error> {
        let (encoded, start) = UVlq32(value).encode_right_justified();
        bytes
            .get_mut(..MAX_LEN)
            .ok_or(Error::OutOfBoundsWriteRawSlice)?
            .copy_from_slice(&encoded);
        Ok(&mut bytes[start..])
    }
}

impl SerializeShrinkWrap for UVlq32Backfill {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.write_raw_slice(&UVlq32(self.0).encode_right_justified().0)
    }
}

#[cfg(feature = "std")]
impl crate::SerializeShrinkWrapOwned for UVlq32Backfill {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn ser_shrink_wrap_owned(&self, wr: &mut crate::BufWriterOwned) -> Result<(), Error> {
        wr.write_raw_slice(&UVlq32(self.0).encode_right_justified().0)
    }
}

impl<'i> DeserializeShrinkWrap<'i> for UVlq32Backfill {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        Ok(UVlq32Backfill(UVlq32::read_forward(rd)?.0))
    }
}

impl DeserializeShrinkWrapOwned for UVlq32Backfill {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, Error> {
        Ok(UVlq32Backfill(UVlq32::read_forward(rd)?.0))
    }
}

impl From<UVlq32Backfill> for u32 {
    fn from(value: UVlq32Backfill) -> Self {
        value.0
    }
}

impl From<u32> for UVlq32Backfill {
    fn from(num: u32) -> Self {
        UVlq32Backfill(num)
    }
}

impl Debug for UVlq32Backfill {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Display for UVlq32Backfill {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod test {
    use crate::vlq32::{UVlq32, UVlq32Backfill};
    use crate::{
        BufReader, BufWriter, DeserializeShrinkWrap, DeserializeShrinkWrapOwned, Error,
        SerializeShrinkWrap,
    };

    #[inline]
    fn test_forward(num: u32, repr: &[u8]) {
        const SIZE: usize = 8;
        let mut buf = [0u8; SIZE];
        let mut wr = BufWriter::new(&mut buf);
        UVlq32(num).write_forward(&mut wr).unwrap();
        assert_eq!(SIZE - wr.bytes_left(), UVlq32(num).len_bytes());
        let buf = wr.finish().unwrap();
        assert_eq!(buf, repr);
        let mut rd = BufReader::new(buf);
        assert_eq!(UVlq32::read_forward(&mut rd), Ok(UVlq32(num)));
        assert_eq!(rd.bytes_left(), 0);
    }

    #[test]
    fn uvlq32_sanity_check() {
        // Examples from https://en.wikipedia.org/wiki/Variable-length_quantity
        test_forward(0, &[0x00]);
        test_forward(0x7f, &[0x7f]);
        test_forward(0x80, &[0x81, 0x00]);
        test_forward(0x2000, &[0xc0, 0x00]);
        test_forward(0x3fff, &[0xff, 0x7f]);
        test_forward(0x4000, &[0x81, 0x80, 0x00]);
        test_forward(0x1f_ffff, &[0xff, 0xff, 0x7f]);
        test_forward(0x20_0000, &[0x81, 0x80, 0x80, 0x00]);
        test_forward(0x800_0000, &[0xc0, 0x80, 0x80, 0x00]);
        test_forward(0xfff_ffff, &[0xff, 0xff, 0xff, 0x7f]);
        test_forward(0x1000_0000, &[0x81, 0x80, 0x80, 0x80, 0x00]);
        test_forward(u32::MAX, &[0x8f, 0xff, 0xff, 0xff, 0x7f]);
    }

    #[test]
    fn len_bytes_sanity() {
        for (bits, len) in [
            (0, 1),
            (7, 1),
            (8, 2),
            (14, 2),
            (15, 3),
            (28, 4),
            (29, 5),
            (32, 5),
        ] {
            let max = if bits == 32 {
                u32::MAX
            } else {
                (1u32 << bits) - 1
            };
            assert_eq!(UVlq32(max).len_bytes(), len, "bits: {bits}");
        }
    }

    #[test]
    fn non_canonical_leading_zero_groups() {
        let buf = [0x80, 0x80, 0x01];
        let mut rd = BufReader::new(&buf);
        assert_eq!(UVlq32::read_forward(&mut rd), Ok(UVlq32(1)));
    }

    #[test]
    fn u32_overflow() {
        // 33 bits
        let buf = [0x9f, 0xff, 0xff, 0xff, 0x7f];
        let mut rd = BufReader::new(&buf);
        assert_eq!(UVlq32::read_forward(&mut rd), Err(Error::MalformedUVlq32));
    }

    #[test]
    fn more_bytes_than_expected() {
        let buf = [0x80, 0x80, 0x80, 0x80, 0x80, 0x01];
        let mut rd = BufReader::new(&buf);
        assert_eq!(UVlq32::read_forward(&mut rd), Err(Error::MalformedUVlq32));
        assert_eq!(rd.bytes_left(), 1);
    }

    #[test]
    fn out_of_bounds() {
        let buf = [0x81, 0x80];
        let mut rd = BufReader::new(&buf);
        assert_eq!(UVlq32::read_forward(&mut rd), Err(Error::OutOfBoundsReadU8));
    }

    #[test]
    fn byte_aligned() {
        let mut buf = [0u8; 4];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_bool(true).unwrap();
        wr.write_uvlq32(0x80).unwrap();
        let bytes = wr.finish().unwrap();
        assert_eq!(bytes, &[0x80, 0x81, 0x00]);
        let mut rd = BufReader::new(bytes);
        assert_eq!(rd.read_bool(), Ok(true));
        assert_eq!(rd.read_uvlq32(), Ok(0x80));
    }

    #[test]
    fn traits() {
        let mut buf = [0; 64];
        let bytes = UVlq32(123_456).to_ww_bytes(&mut buf).unwrap();
        let num = UVlq32::from_ww_bytes(bytes).unwrap();
        assert_eq!(num, UVlq32(123_456));
        let num = UVlq32::from_ww_bytes_owned(bytes).unwrap();
        assert_eq!(num, UVlq32(123_456));

        assert_eq!(UVlq32(0).to_ww_bytes(&mut buf).unwrap(), &[0x00]);
        assert_eq!(UVlq32(127).to_ww_bytes(&mut buf).unwrap(), &[0x7f]);
        assert_eq!(UVlq32(128).to_ww_bytes(&mut buf).unwrap(), &[0x81, 0x00]);
        assert_eq!(
            UVlq32(u32::MAX).to_ww_bytes(&mut buf).unwrap(),
            &[0x8f, 0xff, 0xff, 0xff, 0x7f]
        );
    }

    #[cfg(feature = "std")]
    #[test]
    fn owned_writer_matches() {
        for num in [
            0,
            1,
            0x7f,
            0x80,
            0x3fff,
            0x4000,
            0xfff_ffff,
            0x1000_0000,
            u32::MAX,
        ] {
            let mut buf = [0u8; 8];
            let mut wr = BufWriter::new(&mut buf);
            wr.write_bool(true).unwrap();
            wr.write_uvlq32(num).unwrap();
            let expected = wr.finish().unwrap().to_vec();

            let mut wro = crate::BufWriterOwned::new();
            wro.write_bool(true).unwrap();
            wro.write_uvlq32(num).unwrap();
            assert_eq!(wro.finish().unwrap(), expected, "num: {num}");
        }
    }

    #[test]
    fn backfill_padded() {
        let mut buf = [0u8; 8];
        let bytes = UVlq32Backfill(0).to_ww_bytes(&mut buf).unwrap();
        assert_eq!(bytes, &[0x80, 0x80, 0x80, 0x80, 0x00]);
        let bytes = UVlq32Backfill(0x80).to_ww_bytes(&mut buf).unwrap();
        assert_eq!(bytes, &[0x80, 0x80, 0x80, 0x81, 0x00]);
        assert_eq!(UVlq32::from_ww_bytes(bytes), Ok(UVlq32(0x80)));
        assert_eq!(
            UVlq32Backfill::from_ww_bytes(bytes),
            Ok(UVlq32Backfill(0x80))
        );
        let bytes = UVlq32Backfill(u32::MAX).to_ww_bytes(&mut buf).unwrap();
        assert_eq!(bytes, &[0x8f, 0xff, 0xff, 0xff, 0x7f]);
    }

    #[test]
    fn backfill() {
        for num in [
            0,
            1,
            0x7f,
            0x80,
            0x3fff,
            0x4000,
            0xfff_ffff,
            0x1000_0000,
            u32::MAX,
        ] {
            let mut buf = [0u8; 16];
            let mut wr = BufWriter::new(&mut buf);
            wr.write(&UVlq32Backfill(0)).unwrap();
            wr.write_u8(0xAA).unwrap();
            let len = wr.finish().unwrap().len();
            let bytes = &mut buf[..len];

            let sliced = UVlq32Backfill::backfill(bytes, num).unwrap();
            assert_eq!(sliced.len(), UVlq32(num).len_bytes() + 1);
            let mut rd = BufReader::new(sliced);
            assert_eq!(rd.read_uvlq32(), Ok(num));
            assert_eq!(rd.read_u8(), Ok(0xAA));

            // whole buffer is valid as well
            let mut rd = BufReader::new(bytes);
            assert_eq!(rd.read_uvlq32(), Ok(num));
            assert_eq!(rd.read_u8(), Ok(0xAA));
        }
    }

    #[test]
    fn backfill_short_buffer() {
        let mut buf = [0u8; 4];
        assert_eq!(
            UVlq32Backfill::backfill(&mut buf, 1).map(|s| s.len()),
            Err(Error::OutOfBoundsWriteRawSlice)
        );
    }

    #[cfg(feature = "std")]
    #[test]
    fn backfill_owned_writer_matches() {
        let mut buf = [0u8; 8];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_bool(true).unwrap();
        wr.write(&UVlq32Backfill(1234)).unwrap();
        let expected = wr.finish().unwrap().to_vec();

        let mut wro = crate::BufWriterOwned::new();
        wro.write_bool(true).unwrap();
        wro.write(&UVlq32Backfill(1234)).unwrap();
        assert_eq!(wro.finish().unwrap(), expected);
    }
}
